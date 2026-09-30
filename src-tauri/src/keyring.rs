//! S0.4 钥匙串层：API Key 只存系统凭据管理器（Windows Credential Manager），
//! macOS 上自动落 Keychain，Linux 落 Secret Service。
//!
//! key = "suiyi" + 服务 ID，值 = 明文 API Key（由 OS 负责加密存储）。
//! 任何配置文件、日志、错误信息都不允许携带 Key 明文。

use keyring::Entry;

const SERVICE_NAMESPACE: &str = "suiyi";

fn entry(service_id: &str) -> Result<Entry, String> {
    if service_id.trim().is_empty() {
        return Err("服务 ID 不能为空".into());
    }
    Entry::new(SERVICE_NAMESPACE, service_id).map_err(|e| format!("创建凭据条目失败: {e}"))
}

/// 保存/覆盖 API Key
pub fn set_api_key(service_id: &str, api_key: &str) -> Result<(), String> {
    if api_key.trim().is_empty() {
        return Err("API Key 不能为空".into());
    }
    entry(service_id)?
        .set_password(api_key.trim())
        .map_err(|e| format!("保存密钥失败: {e}"))
}

/// 读取 API Key；不存在时返回 None（不视为错误）
pub fn get_api_key(service_id: &str) -> Result<Option<String>, String> {
    match entry(service_id)?.get_password() {
        Ok(v) => Ok(Some(v)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(format!("读取密钥失败: {e}")),
    }
}

/// 删除 API Key；不存在时静默成功
pub fn delete_api_key(service_id: &str) -> Result<(), String> {
    match entry(service_id)?.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("删除密钥失败: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_get_delete_roundtrip() {
        let id = format!("test-{}", std::process::id());
        // 初始不存在
        assert_eq!(get_api_key(&id).unwrap(), None);
        // 写入 → 读取一致
        set_api_key(&id, "sk-test-123").unwrap();
        assert_eq!(get_api_key(&id).unwrap().as_deref(), Some("sk-test-123"));
        // 覆盖
        set_api_key(&id, "sk-test-456").unwrap();
        assert_eq!(get_api_key(&id).unwrap().as_deref(), Some("sk-test-456"));
        // 删除 → 不存在
        delete_api_key(&id).unwrap();
        assert_eq!(get_api_key(&id).unwrap(), None);
    }

    #[test]
    fn rejects_empty_inputs() {
        assert!(set_api_key("x", "").is_err());
        assert!(set_api_key("  ", "sk-1").is_err());
    }
}
