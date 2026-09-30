//! S0.5 命令层：前端 ↔ 后端的唯一通道
//!
//! 约定：
//! - 返回 Result<T, String>，错误信息可直接展示给用户（不携带密钥）；
//! - 所有涉及 services.json 的命令都即时落盘（写临时文件再改名）；
//! - API Key 相关命令只与系统凭据管理器交互。

use crate::config::{
    load_services, new_service_id, save_services, ServiceConfig, ServiceKind, ServicesFile,
    ResultType, DEFAULT_INPUT_TARGET_LANG, DICTIONARY_PROMPT, INPUT_TARGET_LANGS,
};
use crate::keyring;
use crate::history::{self, HistoryEntry, NewEntry};
use crate::translator::{self, DonePayload, TranslateParams};
use std::path::PathBuf;
use std::time::Duration;
use tauri::Manager;

/// 全局设置（并发数 / 超时）也保存在 services.json，单独读写根结构
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobalSettings {
    pub concurrency: u32,
    pub timeout_secs: u64,
    /// 输入框转译的目标语言
    pub input_target_lang: String,
    /// AnkiConnect 地址与目标牌组
    pub anki_url: String,
    pub anki_deck: String,
}

pub(crate) fn config_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_config_dir()
        .map_err(|e| format!("获取配置目录失败: {e}"))
}

/// 列出全部服务（含全局设置；首次启动时落盘默认内容）
#[tauri::command]
pub fn list_services(app: tauri::AppHandle) -> Result<ServicesFile, String> {
    let dir = config_dir(&app)?;
    let file = load_services(&dir)?;
    save_services(&dir, &file)?; // 首次启动把默认内容固化到磁盘
    Ok(file)
}

/// 新增或更新服务（id 为空视为新增，自动分配 ID 与排序尾位）
#[tauri::command]
pub fn save_service(
    app: tauri::AppHandle,
    mut service: ServiceConfig,
) -> Result<Vec<ServiceConfig>, String> {
    let dir = config_dir(&app)?;
    let mut file = load_services(&dir)?;
    if service.id.trim().is_empty() {
        service.id = new_service_id();
        service.order = file.services.iter().map(|s| s.order).max().unwrap_or(0) + 1;
        file.services.push(service);
    } else if let Some(slot) = file.services.iter_mut().find(|s| s.id == service.id) {
        *slot = service;
    } else {
        return Err(format!("服务不存在: {}", service.id));
    }
    save_services(&dir, &file)?;
    Ok(file.services)
}

/// 删除服务（同时清理凭据管理器中的 Key）
#[tauri::command]
pub fn delete_service(app: tauri::AppHandle, service_id: String) -> Result<Vec<ServiceConfig>, String> {
    let dir = config_dir(&app)?;
    let mut file = load_services(&dir)?;
    let before = file.services.len();
    file.services.retain(|s| s.id != service_id);
    if file.services.len() == before {
        return Err(format!("服务不存在: {service_id}"));
    }
    save_services(&dir, &file)?;
    let _ = keyring::delete_api_key(&service_id); // 不存在时静默
    Ok(file.services)
}

/// 拖拽排序：按前端传来的 ID 顺序重排 order
#[tauri::command]
pub fn reorder_services(app: tauri::AppHandle, ids: Vec<String>) -> Result<Vec<ServiceConfig>, String> {
    let dir = config_dir(&app)?;
    let mut file = load_services(&dir)?;
    for (idx, id) in ids.iter().enumerate() {
        if let Some(s) = file.services.iter_mut().find(|s| &s.id == id) {
            s.order = idx as u32;
        }
    }
    file.services.sort_by_key(|s| s.order);
    save_services(&dir, &file)?;
    Ok(file.services)
}

/// 保存/覆盖某服务的 API Key（只进凭据管理器）
#[tauri::command]
pub fn set_api_key(service_id: String, api_key: String) -> Result<(), String> {
    keyring::set_api_key(&service_id, &api_key)
}

/// 读取某服务的 API Key 是否存在；onlyCheck=true 时不回传明文
#[tauri::command]
pub fn get_api_key(service_id: String, only_check: bool) -> Result<Option<String>, String> {
    let key = keyring::get_api_key(&service_id)?;
    if only_check {
        return Ok(key.map(|_| "••••".into()));
    }
    Ok(key)
}

/// 删除某服务的 API Key
#[tauri::command]
pub fn delete_api_key(service_id: String) -> Result<(), String> {
    keyring::delete_api_key(&service_id)
}

/// 读取全局设置
#[tauri::command]
pub fn get_settings(app: tauri::AppHandle) -> Result<GlobalSettings, String> {
    let file = load_services(&config_dir(&app)?)?;
    Ok(GlobalSettings {
        concurrency: file.concurrency,
        timeout_secs: file.timeout_secs,
        input_target_lang: file.input_target_lang,
        anki_url: file.anki_url,
        anki_deck: file.anki_deck,
    })
}

/// 保存全局设置
#[tauri::command]
pub fn save_settings(app: tauri::AppHandle, settings: GlobalSettings) -> Result<(), String> {
    let dir = config_dir(&app)?;
    let mut file = load_services(&dir)?;
    file.concurrency = settings.concurrency.clamp(1, 8);
    file.timeout_secs = settings.timeout_secs.clamp(3, 120);
    // 只接受白名单内的语言，脏数据一律回落到默认值
    file.input_target_lang = if INPUT_TARGET_LANGS.contains(&settings.input_target_lang.as_str()) {
        settings.input_target_lang
    } else {
        DEFAULT_INPUT_TARGET_LANG.to_string()
    };
    file.anki_url = settings.anki_url.trim().to_string();
    file.anki_deck = settings.anki_deck.trim().to_string();
    save_services(&dir, &file)
}

/// 用指定（已启用）服务翻译文本；流式增量通过 translate-delta / translate-done 事件推送
#[tauri::command]
pub async fn translate_text(
    app: tauri::AppHandle,
    service_id: String,
    text: String,
    from: String,
    to: String,
    // 调用来源：selection / screenshot / manual / input，用于历史归类
    kind: Option<String>,
) -> Result<DonePayload, String> {
    let dir = config_dir(&app)?;
    let file = load_services(&dir)?;
    let service_name = file
        .services
        .iter()
        .find(|s| s.id == service_id)
        .map(|s| s.name.clone())
        .unwrap_or_default();
    let svc = file
        .services
        .iter()
        .find(|s| s.id == service_id)
        .ok_or_else(|| format!("服务不存在: {service_id}"))?;
    if !svc.enabled {
        let msg = "该服务未启用".to_string();
        record_history(&dir, kind.as_deref(), &text, "", &service_name, 0, Some(&msg));
        return Err(msg);
    }
    if svc.kind != ServiceKind::Translation {
        let msg = "该服务不是翻译服务".to_string();
        record_history(&dir, kind.as_deref(), &text, "", &service_name, 0, Some(&msg));
        return Err(msg);
    }
    let key = match keyring::get_api_key(&service_id)? {
        Some(k) => k,
        None => {
            let msg = "该服务尚未设置 API Key，请到设置页填写".to_string();
            record_history(&dir, kind.as_deref(), &text, "", &service_name, 0, Some(&msg));
            return Err(msg);
        }
    };
    // 词典结构化：没有自定义模板时用内置的 JSON 模板，并且强制非流式，
    // 否则拿不到完整 JSON 就没法解析。
    let dictionary_mode = svc.result_type == ResultType::Dictionary;
    let template = if dictionary_mode && svc.prompt_template.is_none() {
        Some(DICTIONARY_PROMPT)
    } else {
        svc.prompt_template.as_deref()
    };
    let prompt = translator::build_prompt(template, &from, &to, &text);
    let params = TranslateParams {
        service_id: &service_id,
        base_url: &svc.base_url,
        api_key: &key,
        model: &svc.model,
        prompt: &prompt,
        temperature: svc.temperature,
        stream: svc.stream && !dictionary_mode,
        timeout: Duration::from_secs(file.timeout_secs.max(3)),
    };
    let (translated, elapsed_ms) = match translator::translate(&app, params).await {
        Ok(v) => v,
        Err(e) => {
            record_history(&dir, kind.as_deref(), &text, "", &service_name, 0, Some(&e));
            return Err(e);
        }
    };
    record_history(
        &dir,
        kind.as_deref(),
        &text,
        &translated,
        &service_name,
        elapsed_ms as i64,
        None,
    );
    Ok(DonePayload {
        service_id,
        dictionary: if dictionary_mode {
            translator::parse_dictionary(&translated)
        } else {
            None
        },
        // 解析失败时保留模型原始输出，前端回退为纯文本展示
        text: translated,
        elapsed_ms,
    })
}

/// 记一条历史。失败只写日志，不能影响翻译本身。
fn record_history(
    dir: &std::path::Path,
    kind: Option<&str>,
    source: &str,
    translated: &str,
    service_name: &str,
    elapsed_ms: i64,
    error: Option<&str>,
) {
    let entry = NewEntry {
        kind: kind.unwrap_or("manual").to_string(),
        source: source.to_string(),
        translated: translated.to_string(),
        service_name: service_name.to_string(),
        elapsed_ms,
        ok: error.is_none(),
        error: error.map(String::from),
    };
    if let Err(e) = history::record(dir, entry) {
        crate::selection::log_line(&format!("history: 写入失败 {e}"));
    }
}

/// 查询历史：query 在原文与译文中模糊匹配，kind 按来源过滤
#[tauri::command]
pub async fn list_history(
    app: tauri::AppHandle,
    query: Option<String>,
    kind: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
) -> Result<Vec<HistoryEntry>, String> {
    let dir = config_dir(&app)?;
    let (limit, offset) = (limit.unwrap_or(100), offset.unwrap_or(0));
    tauri::async_runtime::spawn_blocking(move || history::list(&dir, query, kind, limit, offset))
        .await
        .map_err(|e| format!("查询历史失败: {e}"))?
}

/// 删除单条历史
#[tauri::command]
pub async fn delete_history(app: tauri::AppHandle, id: i64) -> Result<(), String> {
    let dir = config_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || history::delete(&dir, id))
        .await
        .map_err(|e| format!("删除历史失败: {e}"))?
}

/// 清空历史
#[tauri::command]
pub async fn clear_history(app: tauri::AppHandle) -> Result<(), String> {
    let dir = config_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || history::clear(&dir))
        .await
        .map_err(|e| format!("清空历史失败: {e}"))?
}

/// 测试连接的结果。字段可直接展示给用户，不含密钥。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionTest {
    pub ok: bool,
    pub elapsed_ms: u64,
    /// 成功时返回的模型 id 列表（服务未提供则为空）
    pub models: Vec<String>,
    /// 失败原因
    pub error: Option<String>,
}

/// 探测服务连通性：请求 `{baseUrl}/models`，回答三件事——
/// Key 是否有效、网关是否可达、模型是否在权限范围内。
#[tauri::command]
pub async fn test_connection(
    app: tauri::AppHandle,
    service_id: String,
) -> Result<ConnectionTest, String> {
    let file = load_services(&config_dir(&app)?)?;
    let svc = file
        .services
        .iter()
        .find(|s| s.id == service_id)
        .ok_or_else(|| format!("服务不存在: {service_id}"))?;

    let fail = |ms: u64, msg: String| ConnectionTest {
        ok: false,
        elapsed_ms: ms,
        models: Vec::new(),
        error: Some(msg),
    };

    if svc.base_url.trim().is_empty() {
        return Ok(fail(0, "请先填写 Base URL".into()));
    }
    let Some(key) = keyring::get_api_key(&service_id)? else {
        return Ok(fail(0, "请先设置 API Key".into()));
    };

    let url = format!("{}/models", svc.base_url.trim_end_matches('/'));
    let timeout = Duration::from_secs(file.timeout_secs.clamp(3, 120).min(15));
    let client = reqwest::Client::new();
    let started = std::time::Instant::now();
    let resp = tokio::time::timeout(
        timeout,
        client
            .get(&url)
            .header("Authorization", format!("Bearer {key}"))
            .send(),
    )
    .await;
    let elapsed_ms = started.elapsed().as_millis() as u64;

    match resp {
        Err(_) => Ok(fail(
            elapsed_ms,
            format!("连接超时（{}s）", timeout.as_secs()),
        )),
        Ok(Err(e)) => Ok(fail(elapsed_ms, format!("连接失败: {e}"))),
        Ok(Ok(r)) => {
            let status = r.status();
            let body = r.text().await.unwrap_or_default();
            if !status.is_success() {
                let short: String = body.chars().take(200).collect();
                return Ok(fail(elapsed_ms, format!("服务返回 {status}: {short}")));
            }
            Ok(ConnectionTest {
                ok: true,
                elapsed_ms,
                models: parse_model_ids(&body),
                error: None,
            })
        }
    }
}

/// 从 /models 响应里挑出模型 id，兼容 `data[].id`、`models[].slug`、`models[].id`
fn parse_model_ids(body: &str) -> Vec<String> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(body) else {
        return Vec::new();
    };
    for key in ["data", "models"] {
        if let Some(arr) = v[key].as_array() {
            let ids: Vec<String> = arr
                .iter()
                .filter_map(|m| {
                    ["id", "slug", "name"]
                        .iter()
                        .find_map(|k| m[*k].as_str())
                        .map(String::from)
                })
                .collect();
            if !ids.is_empty() {
                return ids;
            }
        }
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::parse_model_ids;

    #[test]
    fn parses_openai_and_zai_model_shapes() {
        let openai = r#"{"object":"list","data":[{"id":"gpt-4o"},{"id":"gpt-4o-mini"}]}"#;
        assert_eq!(parse_model_ids(openai), vec!["gpt-4o", "gpt-4o-mini"]);

        let zai = r#"{"models":[{"slug":"glm-5.3-flash"},{"slug":"glm-4-flash"}]}"#;
        assert_eq!(parse_model_ids(zai), vec!["glm-5.3-flash", "glm-4-flash"]);
    }

    #[test]
    fn tolerates_unexpected_bodies() {
        assert!(parse_model_ids("not json").is_empty());
        assert!(parse_model_ids("{}").is_empty());
    }
}
