//! S0.7 翻译内核：OpenAI 兼容 /chat/completions 适配器（流式 SSE + 非流式）
//!
//! 流式：逐块解析 SSE，把每个增量通过 `translate-delta` 事件推给前端，
//! 结束时发 `translate-done`；非流式：一次性返回。
//! 错误信息面向用户展示，绝不包含 API Key。

use futures_util::StreamExt;
use serde::Serialize;
use serde_json::json;
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

/// 全局共享的 HTTP 客户端。连接池按 host 复用 TCP+TLS 连接：
/// 每次翻译都新建 Client 的话，每次都要重付 DNS + TLS 握手（国内网关几十到上百毫秒，
/// 海外更久），HTTP/2 多路复用与 keepalive 也全部失效。
/// 客户端构建时读取一次系统代理；用户改系统代理后重启应用生效。
static HTTP: OnceLock<reqwest::Client> = OnceLock::new();

pub(crate) fn http_client() -> reqwest::Client {
    HTTP.get_or_init(|| {
        reqwest::Client::builder()
            .pool_max_idle_per_host(8)
            .pool_idle_timeout(Duration::from_secs(90))
            .tcp_nodelay(true)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new())
    })
    .clone()
}

/// 启动预热：对已启用翻译服务的网关各发一次轻量 GET，把 DNS、TCP 与 TLS 握手
/// 在后台先建好，用户第一次翻译直接吃现成连接。只求建连不求结果：
/// 404、超时都无所谓，失败静默——预热只是优化，不能变成启动故障。
pub fn prewarm_gateways(dir: &std::path::Path) {
    let Ok(file) = crate::config::load_services(dir) else {
        return;
    };
    let urls: Vec<String> = file
        .services
        .iter()
        .filter(|s| {
            s.enabled
                && s.kind == crate::config::ServiceKind::Translation
                && !s.base_url.trim().is_empty()
        })
        .map(|s| s.base_url.trim().trim_end_matches('/').to_string())
        .collect();
    if urls.is_empty() {
        return;
    }
    let client = http_client();
    tauri::async_runtime::spawn(async move {
        for u in urls {
            let _ = tokio::time::timeout(Duration::from_secs(5), client.get(&u).send()).await;
        }
    });
}

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
    /// 思考模式的请求体附加字段（按提供商映射好的完整对象）；None 不发任何参数
    pub thinking: Option<serde_json::Value>,
}

/// 按提供商把思考设置映射为请求体的附加字段。
/// 「思考开关」是各家私有扩展，不是 OpenAI 标准字段——严格网关收到未知参数会 400，
/// 所以识别不出的提供商一律不发（DeepSeek、Kimi 这类用模型名区分思考，参数无关）。
pub(crate) fn thinking_body_fields(base_url: &str, setting: &str) -> Option<serde_json::Value> {
    let host = base_url
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .split('/')
        .next()
        .unwrap_or("")
        .to_lowercase();
    let on = match setting {
        "on" => true,
        "off" => false,
        _ => return None, // default：跟随模型默认，不发参数
    };
    // 智谱与火山方舟（豆包 1.5+）用 thinking.type；百炼与硅基流动（Qwen3）用 enable_thinking
    if host.ends_with("bigmodel.cn") || host.ends_with("volces.com") {
        Some(json!({ "thinking": { "type": if on { "enabled" } else { "disabled" } } }))
    } else if host.ends_with("maas.aliyuncs.com") || host.contains("siliconflow") {
        Some(json!({ "enable_thinking": on }))
    } else {
        None
    }
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

/// 组装 /chat/completions 请求体。
/// max_tokens 给个与输入相称的上限：正常译文与输入同量级（×2 已留足膨胀），
/// 封顶是为了模型跑偏、复读打转时收敛长尾，不至于一条请求拖满整个超时。
/// 下限 1024：思考型模型（GLM-5.3-flash 等）的思考 token 计入 max_tokens，
/// 下限太小会被思考烧完预算、正文一个字不剩。
fn request_body(p: &TranslateParams<'_>) -> serde_json::Value {
    let max_tokens = (p.prompt.chars().count() * 2 + 128).max(1024);
    let mut body = json!({
        "model": p.model,
        "stream": p.stream,
        "temperature": p.temperature.unwrap_or(0.3),
        "max_tokens": max_tokens,
        "messages": [{ "role": "user", "content": p.prompt }]
    });
    // 思考模式等提供商私有扩展：按映射结果合并进请求体
    if let Some(extra) = &p.thinking {
        if let Some(map) = extra.as_object() {
            for (k, v) in map {
                body[k] = v.clone();
            }
        }
    }
    body
}

/// 分段阈值的下限：文本超过这么多字符（非流式）才值得并发分段
pub(crate) const SEGMENT_MIN_CHARS: usize = 500;
/// 每段的目标字符数：太小则请求数暴增，太大则单段延迟回到未优化状态
pub(crate) const SEGMENT_TARGET_CHARS: usize = 800;

/// 把长文本切成不超过 target 字符的段：换行优先（保留行结构），
/// 行内按句读切，连句读都没有的硬切兜底。各段拼回必须等于原文。
pub(crate) fn segment_text(text: &str, target: usize) -> Vec<String> {
    if text.chars().count() <= target {
        return vec![text.to_string()];
    }

    // 行内先按句读细分成「不超过 target 的小片」，行本身太长时也靠它兜底
    let split_piece = |line: &str| -> Vec<String> {
        if line.chars().count() <= target {
            return vec![line.to_string()];
        }
        let mut pieces: Vec<String> = Vec::new();
        let mut cur = String::new();
        for ch in line.chars() {
            cur.push(ch);
            let len = cur.chars().count();
            // 硬切优先：迟迟等不到句读也不让单段无限膨胀
            if len >= target {
                pieces.push(cur.drain(..).collect());
            } else if matches!(ch, '。' | '！' | '？' | '；' | '.' | '!' | '?' | ';' | '\n')
                && len >= target / 2
            {
                // 句读留在句尾即可断，避免碎片化
                pieces.push(cur.drain(..).collect());
            }
        }
        if !cur.is_empty() {
            pieces.push(cur);
        }
        pieces
    };

    // 按行收集，行累积到接近 target 就封段；行内容先经过句读细分
    let mut chunks: Vec<String> = Vec::new();
    let mut cur = String::new();
    for line in text.split_inclusive('\n') {
        for piece in split_piece(line) {
            if cur.chars().count() + piece.chars().count() > target && !cur.is_empty() {
                chunks.push(std::mem::take(&mut cur));
            }
            cur.push_str(&piece);
            // 单片就超限（理论上 split_piece 已兜底，这里只防边界）——直接封段
            if cur.chars().count() >= target {
                chunks.push(std::mem::take(&mut cur));
            }
        }
    }
    if !cur.is_empty() {
        chunks.push(cur);
    }
    chunks
}

/// 应用内入口（带事件推送）
pub async fn translate(app: &AppHandle, p: TranslateParams<'_>) -> Result<(String, u64), String> {
    translate_inner(Some(app), p).await
}

/// 核心实现；app=None 时不推送事件（供单元测试与分段并发使用）
pub(crate) async fn translate_inner(
    app: Option<&AppHandle>,
    p: TranslateParams<'_>,
) -> Result<(String, u64), String> {
    let start = Instant::now();
    let client = http_client();
    let url = format!("{}/chat/completions", p.base_url.trim_end_matches('/'));
    let body = request_body(&p);

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
        if let Some(msg) = data["error"]["message"].as_str() {
            return Err(format!("服务返回错误: {msg}"));
        }
        let text = data["choices"][0]["message"]["content"]
            .as_str()
            .ok_or("响应缺少 content 字段")?
            .to_string();
        if text.trim().is_empty() {
            return Err(empty_result_error());
        }
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
                    if full.trim().is_empty() {
                        return Err(empty_result_error());
                    }
                    let elapsed = start.elapsed().as_millis() as u64;
                    emit_done(app, p.service_id, &full, elapsed);
                    return Ok((full, elapsed));
                }
                let Ok(v) = serde_json::from_str::<serde_json::Value>(data) else {
                    continue;
                };
                // 服务把错误夹在流里（余额不足、内容审查等）：转发，绝不静默吞掉
                if let Some(msg) = v["error"]["message"].as_str() {
                    return Err(format!("服务返回错误: {msg}"));
                }
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
    if full.trim().is_empty() {
        return Err(empty_result_error());
    }
    emit_done(app, p.service_id, &full, elapsed);
    Ok((full, elapsed))
}

/// 空结果的统一报错文案：给用户下一步动作，而不是一张空白卡片
fn empty_result_error() -> String {
    "模型没有返回内容（可能思考型模型把预算花在了思考上，或输出被截断），请重试；反复出现请换模型".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    /// 全局 HTTP 客户端只在首次取用时构建，之后一直复用同一个：
    /// 连接池、HTTP/2 与 keepalive 才能跨请求生效。
    #[test]
    fn http_client_is_cached_in_the_global_pool() {
        let _first = http_client();
        assert!(HTTP.get().is_some(), "首次取用后应已把客户端缓存进全局槽位");
        let _second = http_client(); // 再次取用走缓存，不重建
    }

    /// 启动预热读不到配置时必须安静返回：预热只是优化，不能变成启动故障
    #[test]
    fn prewarm_tolerates_missing_config_dir() {
        prewarm_gateways(std::path::Path::new("Z:/suiyi/definitely/not/here"));
    }

    /// 分段：长文本按目标块大小切开，换行优先、句读次之、兜底硬切；
    /// 拼回必须等于原文（一行都不能丢），除单段超长的兜底硬切外每块不超限。
    #[test]
    fn segment_text_splits_long_text_and_preserves_everything() {
        // 短文本不切
        assert_eq!(segment_text("hello", 500), vec!["hello".to_string()]);

        // 多行长文本：按行累积成块，每块不超限，拼回等于原文
        let mut long = String::new();
        for i in 0..40 {
            long.push_str(&format!("line {i} with some words here\n"));
        }
        let chunks = segment_text(&long, 300);
        assert!(chunks.len() > 1, "长文本应被切开");
        assert_eq!(chunks.concat(), long, "拼回必须等于原文");
        assert!(chunks.iter().all(|c| c.chars().count() <= 300), "每块不超限");

        // 无换行的长文本：按句读切
        let one_line = "第一句。".repeat(100);
        let chunks = segment_text(&one_line, 120);
        assert!(chunks.len() > 1);
        assert_eq!(chunks.concat(), one_line);
        assert!(chunks.iter().all(|c| c.chars().count() <= 120));

        // 连换行带句读都没有的超长行：硬切兜底，仍要拼得回
        let solid = "a".repeat(1000);
        let chunks = segment_text(&solid, 300);
        assert_eq!(chunks.concat(), solid);
        assert!(chunks.iter().all(|c| c.chars().count() <= 300));

        // 恰好在阈值边上不产生空块
        for c in segment_text("ab\n", 3) {
            assert!(!c.is_empty());
        }
    }

    /// 流式响应一个正文都没有（思考型模型把预算烧在思考通道、或被截断）：
    /// 必须报错说清楚，而不是静默返回空串让用户对着空白卡片猜。
    #[tokio::test]
    async fn empty_stream_surfaces_an_error() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut sock, _) = listener.accept().unwrap();
            let mut req = vec![0u8; 8192];
            let _ = sock.read(&mut req);
            let sse = "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"思考中\"}}]}\n\n\
                       data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"length\"}]}\n\n\
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
            prompt: "translate",
            temperature: None,
            stream: true,
            timeout: Duration::from_secs(5),
            thinking: None,
        };
        let err = translate_inner(None, p).await.unwrap_err();
        assert!(err.contains("没有返回内容"), "实际: {err}");
        server.join().unwrap();
    }

    /// 服务在流里夹带 error 对象（余额不足、内容审查等）：转发为可读错误，
    /// 不允许静默吞掉后返回空文本。
    #[tokio::test]
    async fn stream_error_object_surfaces() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut sock, _) = listener.accept().unwrap();
            let mut req = vec![0u8; 8192];
            let _ = sock.read(&mut req);
            let sse = "data: {\"error\":{\"message\":\"余额不足\"}}\n\n\
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
            prompt: "translate",
            temperature: None,
            stream: true,
            timeout: Duration::from_secs(5),
            thinking: None,
        };
        let err = translate_inner(None, p).await.unwrap_err();
        assert!(err.contains("余额不足"), "实际: {err}");
        server.join().unwrap();
    }

    /// 非流式空正文同样要报错
    #[tokio::test]
    async fn empty_non_stream_surfaces_an_error() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut sock, _) = listener.accept().unwrap();
            let mut req = vec![0u8; 8192];
            let _ = sock.read(&mut req);
            let body = "{\"choices\":[{\"message\":{\"content\":\"\"}}]}";
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
            api_key: "",
            model: "mock-model",
            prompt: "translate",
            temperature: None,
            stream: false,
            timeout: Duration::from_secs(5),
            thinking: None,
        };
        let err = translate_inner(None, p).await.unwrap_err();
        assert!(err.contains("没有返回内容"), "实际: {err}");
        server.join().unwrap();
    }

    /// 思考开关的提供商映射矩阵：智谱/方舟走 thinking.type，百炼/硅基流动走
    /// enable_thinking；识别不出的提供商与 default 档一律不发参数（严格网关会 400）。
    #[test]
    fn thinking_fields_follow_provider_and_setting() {
        // 智谱
        assert_eq!(
            thinking_body_fields("https://open.bigmodel.cn/api/paas/v4", "off"),
            Some(json!({ "thinking": { "type": "disabled" } }))
        );
        assert_eq!(
            thinking_body_fields("https://open.bigmodel.cn/api/paas/v4", "on"),
            Some(json!({ "thinking": { "type": "enabled" } }))
        );
        // 百炼（地址含工作空间变量也不影响主机名判断）
        assert_eq!(
            thinking_body_fields(
                "https://x.cn-beijing.maas.aliyuncs.com/compatible-mode/v1",
                "off"
            ),
            Some(json!({ "enable_thinking": false }))
        );
        // 方舟
        assert_eq!(
            thinking_body_fields("https://ark.cn-beijing.volces.com/api/v3", "off"),
            Some(json!({ "thinking": { "type": "disabled" } }))
        );
        // 硅基流动
        assert_eq!(
            thinking_body_fields("https://api.siliconflow.cn/v1", "on"),
            Some(json!({ "enable_thinking": true }))
        );
        // 识别不出的提供商：不发（OpenAI 官方等严格网关会拒绝未知字段）
        assert_eq!(thinking_body_fields("https://api.deepseek.com", "off"), None);
        // default 档：跟随模型默认，不发
        assert_eq!(
            thinking_body_fields("https://open.bigmodel.cn/api/paas/v4", "default"),
            None
        );
    }

    /// request_body 把思考附加字段合并进请求体；None 时不含该字段
    #[test]
    fn request_body_merges_thinking_fields() {
        let p = TranslateParams {
            service_id: "s",
            base_url: "http://localhost:9",
            api_key: "",
            model: "m",
            prompt: "hi",
            temperature: None,
            stream: true,
            timeout: Duration::from_secs(1),
            thinking: Some(json!({ "thinking": { "type": "disabled" } })),
        };
        let body = request_body(&p);
        assert_eq!(body["thinking"]["type"], "disabled");
        assert_eq!(body["stream"], true, "原有字段不受合并影响");

        let p2 = TranslateParams { thinking: None, ..p };
        assert!(request_body(&p2)["thinking"].is_null(), "None 时不发思考字段");
    }

    /// 请求体带与输入相称的 max_tokens 上限：正常译文与输入同量级，
    /// 封顶是为了模型跑偏复读时收敛长尾，不让一条请求拖满超时。
    /// 下限 1024：给思考型模型的思考通道留出预算（思考 token 计入 max_tokens），
    /// 否则短文本翻译会被思考烧完预算、正文为空。
    #[test]
    fn request_body_caps_max_tokens_relative_to_input() {
        let p = TranslateParams {
            service_id: "s",
            base_url: "http://localhost:9",
            api_key: "",
            model: "m",
            prompt: "hi",
            temperature: None,
            stream: false,
            timeout: Duration::from_secs(1),
            thinking: None,
        };
        let body = request_body(&p);
        assert_eq!(body["max_tokens"], 1024, "短输入也给足下限，覆盖思考通道预算");
        let long = "a".repeat(1000);
        let p2 = TranslateParams { prompt: &long, ..p };
        let body2 = request_body(&p2);
        assert_eq!(body2["max_tokens"], 2128, "1000 字符输入 → 2×1000+128");
        assert_eq!(body2["stream"], false);
        assert_eq!(body2["messages"][0]["content"], "a".repeat(1000));
    }

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
            thinking: None,
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
            thinking: None,
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
            thinking: None,
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
                    thinking: thinking_body_fields(&base, &svc.thinking),
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
