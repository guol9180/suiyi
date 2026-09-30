//! S0.5 命令层：前端 ↔ 后端的唯一通道
//!
//! 约定：
//! - 返回 Result<T, String>，错误信息可直接展示给用户（不携带密钥）；
//! - 所有涉及 services.json 的命令都即时落盘（写临时文件再改名）；
//! - API Key 相关命令只与系统凭据管理器交互。

use crate::config::{
    load_services, new_service_id, save_services, ServiceConfig, ServicesFile,
};
use crate::keyring;
use std::path::PathBuf;
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
