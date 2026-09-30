//! S0.5 命令层：前端 ↔ 后端的唯一通道
//!
//! 约定：
//! - 返回 Result<T, String>，错误信息可直接展示给用户（不携带密钥）；
//! - 所有涉及 services.json 的命令都即时落盘（写临时文件再改名）；
//! - API Key 相关命令只与系统凭据管理器交互。

use crate::config::{
    load_services, new_service_id, save_services, ServiceConfig, ServiceKind, ServicesFile,
};
use crate::keyring;
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
}

fn config_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
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
    })
}

/// 保存全局设置
#[tauri::command]
pub fn save_settings(app: tauri::AppHandle, settings: GlobalSettings) -> Result<(), String> {
    let dir = config_dir(&app)?;
    let mut file = load_services(&dir)?;
    file.concurrency = settings.concurrency.clamp(1, 8);
    file.timeout_secs = settings.timeout_secs.clamp(3, 120);
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
) -> Result<DonePayload, String> {
    let file = load_services(&config_dir(&app)?)?;
    let svc = file
        .services
        .iter()
        .find(|s| s.id == service_id)
        .ok_or_else(|| format!("服务不存在: {service_id}"))?;
    if !svc.enabled {
        return Err("该服务未启用".into());
    }
    if svc.kind != ServiceKind::Translation {
        return Err("该服务不是翻译服务".into());
    }
    let key = keyring::get_api_key(&service_id)?
        .ok_or("该服务尚未设置 API Key，请到设置页填写")?;
    let prompt = translator::build_prompt(svc.prompt_template.as_deref(), &from, &to, &text);
    let params = TranslateParams {
        service_id: &service_id,
        base_url: &svc.base_url,
        api_key: &key,
        model: &svc.model,
        prompt: &prompt,
        temperature: svc.temperature,
        stream: svc.stream,
        timeout: Duration::from_secs(file.timeout_secs.max(3)),
    };
    let (text, elapsed_ms) = translator::translate(&app, params).await?;
    Ok(DonePayload {
        service_id,
        text,
        elapsed_ms,
    })
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
