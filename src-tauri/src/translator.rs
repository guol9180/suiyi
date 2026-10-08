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
    /// 词典结构化结果；纯文本服务或无解析结果时为 None
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dictionary: Option<DictionaryResult>,
}

/// 词典释义的一条义项
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Sense {
    pub pos: String,
    pub def: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub example: Option<String>,
}

/// 结构化词条
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DictionaryResult {
    pub word: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phonetic: Option<String>,
    pub senses: Vec<Sense>,
}

/// 从模型输出里解析词典 JSON。
/// 容忍 ```json 代码块、前后夹带的说明文字；解析不出有效义项时返回 None，
/// 由调用方回退为纯文本展示。
pub fn parse_dictionary(raw: &str) -> Option<DictionaryResult> {
    let start = raw.find('{')?;
    let end = raw.rfind('}')?;
    if end <= start {
        return None;
    }
    let v: serde_json::Value = serde_json::from_str(&raw[start..=end]).ok()?;

    let word = v["word"].as_str().unwrap_or_default().trim().to_string();
    let phonetic = v["phonetic"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from);

    let senses: Vec<Sense> = v["senses"]
        .as_array()?
        .iter()
        .filter_map(|s| {
            let def = s["def"].as_str()?.trim();
            if def.is_empty() {
                return None;
            }
            Some(Sense {
                pos: s["pos"].as_str().unwrap_or_default().trim().to_string(),
                def: def.to_string(),
                example: s["example"]
                    .as_str()
                    .map(str::trim)
                    .filter(|e| !e.is_empty())
                    .map(String::from),
            })
        })
        .collect();

    if senses.is_empty() {
        return None;
    }
    Some(DictionaryResult {
        word: if word.is_empty() { "词条".into() } else { word },
        phonetic,
        senses,
    })
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
                // 结构化解析在命令层完成，事件只负责通知文本已完成
                dictionary: None,
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

    // 本地服务（Ollama 等）没有密钥：不带 Authorization 头，
    // 而不是发一个空的 Bearer —— 后者会被部分网关直接判 401。
    let mut req = client.post(&url).header("Content-Type", "application/json");
    if !p.api_key.trim().is_empty() {
        req = req.header("Authorization", format!("Bearer {}", p.api_key.trim()));
    }

    let resp = tokio::time::timeout(p.timeout, req.json(&body).send())
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

    /// 本地服务没有密钥：请求里不该出现 Authorization 头。
    /// 发一个空的 `Bearer ` 会被部分网关直接判 401，所以这里盯的是「干脆不发」。
    #[tokio::test]
    async fn omits_authorization_header_when_key_is_empty() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut sock, _) = listener.accept().unwrap();
            let mut req = vec![0u8; 8192];
            let n = sock.read(&mut req).unwrap_or(0);
            let seen = String::from_utf8_lossy(&req[..n]).to_string();
            let body = "{\"choices\":[{\"message\":{\"content\":\"hi\"}}]}";
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{body}"
            );
            let _ = sock.write_all(resp.as_bytes());
            let _ = sock.flush();
            seen
        });

        let base = format!("http://{addr}");
        let p = TranslateParams {
            service_id: "mock",
            base_url: &base,
            api_key: "",
            model: "mock-model",
            prompt: "p",
            temperature: None,
            stream: false,
            timeout: Duration::from_secs(5),
        };
        let (text, _ms) = translate_inner(None, p).await.unwrap();
        assert_eq!(text, "hi");
        let seen = server.join().unwrap();
        assert!(
            !seen.to_lowercase().contains("authorization"),
            "没有密钥时不该带 Authorization 头，实际请求：{seen}"
        );
    }

    #[test]
    fn prompt_template_renders_vars() {
        let t = Some("A{{from}}B{{to}}C{{text}}");
        assert_eq!(build_prompt(t, "EN", "中文", "hi"), "AENB中文Chi");
        // 空模板 → 默认模板（含默认变量壳）
        assert!(build_prompt(None, "EN", "中文", "hi").contains("hi"));
    }

    #[test]
    fn parses_dictionary_payloads() {
        // 干净的 JSON
        let plain = r#"{"word":"retrieval","phonetic":"/rɪˈtriːvl/",
            "senses":[{"pos":"n.","def":"检索；找回","example":"efficient retrieval — 高效检索"},
                      {"pos":"n.","def":"数据读取"}]}"#;
        let d = parse_dictionary(plain).expect("应能解析");
        assert_eq!(d.word, "retrieval");
        assert_eq!(d.phonetic.as_deref(), Some("/rɪˈtriːvl/"));
        assert_eq!(d.senses.len(), 2);
        assert_eq!(d.senses[0].def, "检索；找回");
        assert!(d.senses[1].example.is_none());

        // 夹带代码块与说明文字
        let wrapped = "好的，结果如下：\n```json\n{\"word\":\"hi\",\"senses\":[{\"pos\":\"int.\",\"def\":\"你好\"}]}\n```\n希望有帮助";
        let d = parse_dictionary(wrapped).expect("应能容错解析");
        assert_eq!(d.word, "hi");
        assert!(d.phonetic.is_none());

        // 不是 JSON / 没有义项 → None，由调用方回退纯文本
        assert!(parse_dictionary("就是一段普通译文").is_none());
        assert!(parse_dictionary(r#"{"word":"x","senses":[]}"#).is_none());
        assert!(parse_dictionary(r#"{"word":"x","senses":[{"pos":"n.","def":"  "}]}"#).is_none());
    }

    /// 真实服务冒烟验收：读取本机 services.json + 凭据管理器中已启用的服务，
    /// 发送一句真实翻译。平时被 #[ignore] 跳过，显式运行：
    ///   cargo test real_service_smoke -- --ignored --nocapture
    #[test]
    #[ignore]
    fn real_service_smoke() {
        let appdata = std::env::var("APPDATA").expect("APPDATA 未定义");
        let dir = std::path::Path::new(&appdata).join("com.suiyi.dev");
        let file = crate::config::load_services(&dir).expect("读取配置失败");
        let svc = file
            .services
            .iter()
            .find(|s| s.enabled)
            .expect("没有已启用的服务");
        let key = crate::keyring::get_api_key(&svc.id)
            .expect("读取凭据失败")
            .expect("该服务没有保存 API Key");
        // 可用 SUIYI_SMOKE_MODEL / SUIYI_SMOKE_BASE 覆盖模型与网关（排查用）
        let model = std::env::var("SUIYI_SMOKE_MODEL").unwrap_or_else(|_| svc.model.clone());
        let base = std::env::var("SUIYI_SMOKE_BASE").unwrap_or_else(|_| svc.base_url.clone());
        let prompt = build_prompt(svc.prompt_template.as_deref(), "自动检测", "简体中文", "Hello, world!");
        let rt = tokio::runtime::Runtime::new().unwrap();
        let (text, ms) = rt
            .block_on(translate_inner(
                None,
                TranslateParams {
                    service_id: &svc.id,
                    base_url: &base,
                    api_key: &key,
                    model: &model,
                    prompt: &prompt,
                    temperature: svc.temperature,
                    stream: svc.stream,
                    timeout: std::time::Duration::from_secs(file.timeout_secs.max(10)),
                },
            ))
            .expect("真实服务翻译失败");
        assert!(!text.trim().is_empty(), "真实服务返回了空文本");
        println!("SMOKE_OK | 模型={model} | 耗时={ms}ms | 译文: {text}");
    }

    /// 探针：用已存 Key 请求 /models，验证 Key 与平台是否匹配（不打印 Key）
    #[test]
    #[ignore]
    fn real_models_probe() {
        let appdata = std::env::var("APPDATA").unwrap();
        let dir = std::path::Path::new(&appdata).join("com.suiyi.dev");
        let file = crate::config::load_services(&dir).unwrap();
        let svc = file.services.iter().find(|s| s.enabled).expect("no enabled svc");
        let key = crate::keyring::get_api_key(&svc.id)
            .unwrap()
            .expect("no api key");
        let rt = tokio::runtime::Runtime::new().unwrap();
        let out = rt.block_on(async {
            let client = reqwest::Client::new();
            let url = format!("{}/models", svc.base_url.trim_end_matches('/'));
            let resp = client
                .get(&url)
                .header("Authorization", format!("Bearer {key}"))
                .send()
                .await;
            match resp {
                Ok(r) => {
                    let status = r.status();
                    let text = r.text().await.unwrap_or_default();
                    if status.is_success() {
                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                            let slugs: Vec<String> = v["models"]
                                .as_array()
                                .map(|a| {
                                    a.iter()
                                        .filter_map(|m| m["slug"].as_str().map(String::from))
                                        .collect()
                                })
                                .unwrap_or_default();
                            format!("GET /models → {status} | 可用模型: {slugs:?}")
                        } else {
                            format!("GET /models → {status} | 解析失败: {}", text.chars().take(300).collect::<String>())
                        }
                    } else {
                        format!("GET /models → {status} | {}", text.chars().take(300).collect::<String>())
                    }
                }
                Err(e) => format!("GET /models 连接失败: {e}"),
            }
        });
        println!("MODELS_PROBE [{}] {}", svc.base_url, out);
    }
}
