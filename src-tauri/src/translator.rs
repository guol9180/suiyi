//! S0.7 翻译内核：OpenAI 兼容 /chat/completions 适配器（流式 SSE + 非流式）
//!
//! 流式：逐块解析 SSE，把每个增量通过 `translate-delta` 事件推给前端，
//! 结束时发 `translate-done`；非流式：一次性返回。
//! 错误信息面向用户展示，绝不包含 API Key。

use futures_util::StreamExt;
use serde::Serialize;
use serde_json::json;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeltaPayload {
    pub service_id: String,
    pub delta: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DonePayload {
    pub service_id: String,
    pub text: String,
    pub elapsed_ms: u64,
}

pub struct TranslateParams<'a> {
    pub service_id: &'a str,
    pub base_url: &'a str,
    pub api_key: &'a str,
    pub model: &'a str,
    pub prompt: &'a str,
    pub temperature: Option<f32>,
    pub stream: bool,
    pub timeout: Duration,
}

/// 用模板渲染 Prompt（变量：{{from}} {{to}} {{text}}）
pub fn build_prompt(template: Option<&str>, from: &str, to: &str, text: &str) -> String {
    let t = template
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(crate::config::DEFAULT_PROMPT);
    t.replace("{{from}}", from)
        .replace("{{to}}", to)
        .replace("{{text}}", text)
}

fn emit_delta(app: Option<&AppHandle>, service_id: &str, delta: &str) {
    if let Some(app) = app {
        let _ = app.emit(
            "translate-delta",
            DeltaPayload {
                service_id: service_id.to_string(),
                delta: delta.to_string(),
            },
        );
    }
}

fn emit_done(app: Option<&AppHandle>, service_id: &str, text: &str, elapsed_ms: u64) {
    if let Some(app) = app {
        let _ = app.emit(
            "translate-done",
            DonePayload {
                service_id: service_id.to_string(),
                text: text.to_string(),
                elapsed_ms,
            },
        );
    }
}

/// 应用内入口（带事件推送）
pub async fn translate(app: &AppHandle, p: TranslateParams<'_>) -> Result<(String, u64), String> {
    translate_inner(Some(app), p).await
}

/// 核心实现；app=None 时不推送事件（供单元测试使用）
async fn translate_inner(
    app: Option<&AppHandle>,
    p: TranslateParams<'_>,
) -> Result<(String, u64), String> {
    let start = Instant::now();
    let client = reqwest::Client::new();
    let url = format!("{}/chat/completions", p.base_url.trim_end_matches('/'));
    let body = json!({
        "model": p.model,
        "stream": p.stream,
        "temperature": p.temperature.unwrap_or(0.3),
        "messages": [{ "role": "user", "content": p.prompt }]
    });

    let resp = tokio::time::timeout(
        p.timeout,
        client
            .post(&url)
            .header("Authorization", format!("Bearer {}", p.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send(),
    )
    .await
    .map_err(|_| "请求超时".to_string())?
    .map_err(|e| format!("连接失败: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body_text = resp.text().await.unwrap_or_default();
        let short: String = body_text.chars().take(300).collect();
        return Err(format!("服务返回 {status}: {short}"));
    }

    if !p.stream {
        let data: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| format!("解析响应失败: {e}"))?;
        let text = data["choices"][0]["message"]["content"]
            .as_str()
            .ok_or("响应缺少 content 字段")?
            .to_string();
        let elapsed = start.elapsed().as_millis() as u64;
        emit_done(app, p.service_id, &text, elapsed);
        return Ok((text, elapsed));
    }

    // ---- 流式 SSE ----
    let mut stream = resp.bytes_stream();
    let mut buf = String::new();
    let mut full = String::new();

    loop {
        let chunk = match tokio::time::timeout(p.timeout, stream.next()).await {
            Ok(Some(c)) => c.map_err(|e| format!("流中断: {e}"))?,
            Ok(None) => break, // 服务端关闭连接
            Err(_) => return Err("流式读取超时".to_string()),
        };
        buf.push_str(&String::from_utf8_lossy(&chunk).replace("\r\n", "\n"));

        while let Some(pos) = buf.find("\n\n") {
            let event: String = buf.drain(..pos + 2).collect();
            for line in event.lines() {
                let Some(data) = line.strip_prefix("data:") else { continue };
                let data = data.trim();
                if data == "[DONE]" {
                    let elapsed = start.elapsed().as_millis() as u64;
                    emit_done(app, p.service_id, &full, elapsed);
                    return Ok((full, elapsed));
                }
                let Ok(v) = serde_json::from_str::<serde_json::Value>(data) else {
                    continue;
                };
                let Some(delta) = v["choices"][0]["delta"]["content"].as_str() else {
                    continue;
                };
                if delta.is_empty() {
                    continue;
                }
                full.push_str(delta);
                emit_delta(app, p.service_id, delta);
            }
        }
    }

    // 流关闭但没有 [DONE]：把已积累内容视为完成
    let elapsed = start.elapsed().as_millis() as u64;
    emit_done(app, p.service_id, &full, elapsed);
    Ok((full, elapsed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    /// 起一个零依赖的 SSE mock 服务（TcpListener 线程），端到端验证流式解析
    #[tokio::test]
    async fn parses_sse_stream_end_to_end() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut sock, _) = listener.accept().unwrap();
            let mut req = vec![0u8; 8192];
            let _ = sock.read(&mut req); // mock 不校验请求内容
            let sse = "data: {\"choices\":[{\"delta\":{\"content\":\"你好\"}}]}\n\n\
                       data: {\"choices\":[{\"delta\":{\"content\":\"，世界\"}}]}\n\n\
                       data: {\"choices\":[{\"delta\":{}}]}\n\n\
                       data: [DONE]\n\n";
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n{sse}"
            );
            let _ = sock.write_all(resp.as_bytes());
            let _ = sock.flush();
        });

        let base = format!("http://{addr}");
        let p = TranslateParams {
            service_id: "mock",
            base_url: &base,
            api_key: "sk-test",
            model: "mock-model",
            prompt: "translate this",
            temperature: Some(0.2),
            stream: true,
            timeout: Duration::from_secs(5),
        };
        let (text, _ms) = translate_inner(None, p).await.unwrap();
        assert_eq!(text, "你好，世界");
        server.join().unwrap();
    }

    /// 非流式路径
    #[tokio::test]
    async fn parses_non_stream_response() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut sock, _) = listener.accept().unwrap();
            let mut req = vec![0u8; 8192];
            let _ = sock.read(&mut req);
            let body = "{\"choices\":[{\"message\":{\"content\":\"hello world\"}}]}";
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{body}"
            );
            let _ = sock.write_all(resp.as_bytes());
            let _ = sock.flush();
        });

        let base = format!("http://{addr}");
        let p = TranslateParams {
            service_id: "mock",
            base_url: &base,
            api_key: "sk-test",
            model: "mock-model",
            prompt: "p",
            temperature: None,
            stream: false,
            timeout: Duration::from_secs(5),
        };
        let (text, _ms) = translate_inner(None, p).await.unwrap();
        assert_eq!(text, "hello world");
        server.join().unwrap();
    }

    #[test]
    fn prompt_template_renders_vars() {
        let t = Some("A{{from}}B{{to}}C{{text}}");
        assert_eq!(build_prompt(t, "EN", "中文", "hi"), "AENB中文Chi");
        // 空模板 → 默认模板（含默认变量壳）
        assert!(build_prompt(None, "EN", "中文", "hi").contains("hi"));
    }
}
