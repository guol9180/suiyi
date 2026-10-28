//! S0.4 钥匙串层：API Key 只存系统凭据管理器（Windows Credential Manager），
//! macOS 上自动落 Keychain，Linux 落 Secret Service。
//!
//! key = "suiyi" + 服务 ID，值 = 明文 API Key（由 OS 负责加密存储）。
//! 任何配置文件、日志、错误信息都不允许携带 Key 明文。

use keyring::Entry;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

const SERVICE_NAMESPACE: &str = "suiyi";

/// 凭据读取缓存：每次翻译都要读一次 Key，凭据管理器是同步 COM 调用，
/// 进程内缓存掉。写入与删除时同步更新缓存；在系统凭据管理器里绕过应用
/// 手改 Key 的场景极少，重启应用即恢复一致。
struct KeyCache {
    map: HashMap<String, Option<String>>,
}

impl KeyCache {
    fn new() -> Self {
        Self { map: HashMap::new() }
    }
}

fn key_cache() -> &'static Mutex<KeyCache> {
    static CACHE: OnceLock<Mutex<KeyCache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(KeyCache::new()))
}

/// 命中返回缓存；未命中用 loader 读一次并缓存
fn get_or_load(
    cache: &mut KeyCache,
    service_id: &str,
    loader: fn(&str) -> Result<Option<String>, String>,
) -> Result<Option<String>, String> {
    if let Some(v) = cache.map.get(service_id) {
        return Ok(v.clone());
    }
    let v = loader(service_id)?;
    cache.map.insert(service_id.to_string(), v.clone());
    Ok(v)
}

/// 覆盖缓存值
fn put_key(cache: &mut KeyCache, service_id: &str, value: Option<String>) {
    cache.map.insert(service_id.to_string(), value);
}

/// 失效缓存值（下次读取重新加载）。仅测试用：生产路径的删除走 put_key(None)，
/// 把「确认不存在」也缓存住，省得每次翻译都去打一遍凭据管理器
#[cfg(test)]
fn invalidate_key(cache: &mut KeyCache, service_id: &str) {
    cache.map.remove(service_id);
}

fn entry(service_id: &str) -> Result<Entry, String> {
    if service_id.trim().is_empty() {
        return Err("服务 ID 不能为空".into());
    }
    Entry::new(SERVICE_NAMESPACE, service_id).map_err(|e| format!("创建凭据条目失败: {e}"))
}

/// 真实读取（不带缓存），供 get_or_load 的 loader 使用
fn read_from_store(service_id: &str) -> Result<Option<String>, String> {
    match entry(service_id)?.get_password() {
        Ok(v) => Ok(Some(v)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(format!("读取密钥失败: {e}")),
    }
}

/// 保存/覆盖 API Key
pub fn set_api_key(service_id: &str, api_key: &str) -> Result<(), String> {
    if api_key.trim().is_empty() {
        return Err("API Key 不能为空".into());
    }
    entry(service_id)?
        .set_password(api_key.trim())
        .map_err(|e| format!("保存密钥失败: {e}"))?;
    if let Ok(mut c) = key_cache().lock() {
        put_key(&mut c, service_id, Some(api_key.trim().to_string()));
    }
    Ok(())
}

/// 读取 API Key；不存在时返回 None（不视为错误）。走进程内缓存。
pub fn get_api_key(service_id: &str) -> Result<Option<String>, String> {
    let mut c = key_cache().lock().map_err(|_| "凭据缓存锁失败".to_string())?;
    get_or_load(&mut c, service_id, read_from_store)
}

/// 删除 API Key；不存在时静默成功
pub fn delete_api_key(service_id: &str) -> Result<(), String> {
    match entry(service_id)?.delete_credential() {
        Ok(()) => {}
        Err(keyring::Error::NoEntry) => {}
        Err(e) => return Err(format!("删除密钥失败: {e}")),
    }
    if let Ok(mut c) = key_cache().lock() {
        put_key(&mut c, service_id, None);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 缓存语义：未命中加载一次并缓存；put 覆盖缓存；invalidate 之后重新加载。
    /// 每次翻译都要读一次 Key，凭据管理器是 COM 调用，必须进程内缓存掉。
    #[test]
    fn key_cache_loads_once_until_invalidated() {
        use std::sync::atomic::Ordering;
        let mut c = KeyCache::new();
        LOADS.store(0, Ordering::SeqCst);
        assert_eq!(get_or_load(&mut c, "a", fake_load).unwrap().as_deref(), Some("sk-1"));
        assert_eq!(
            get_or_load(&mut c, "a", fake_load).unwrap().as_deref(),
            Some("sk-1"),
            "命中缓存不再触发加载"
        );
        assert_eq!(LOADS.load(Ordering::SeqCst), 1);

        put_key(&mut c, "a", Some("sk-new".into()));
        assert_eq!(get_or_load(&mut c, "a", fake_load).unwrap().as_deref(), Some("sk-new"));

        invalidate_key(&mut c, "a");
        assert_eq!(
            get_or_load(&mut c, "a", fake_load).unwrap().as_deref(),
            Some("sk-2"),
            "失效之后重新加载"
        );
        assert_eq!(LOADS.load(Ordering::SeqCst), 2);
    }

    static LOADS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    fn fake_load(_id: &str) -> Result<Option<String>, String> {
        use std::sync::atomic::Ordering;
        let n = LOADS.fetch_add(1, Ordering::SeqCst) + 1;
        Ok(Some(format!("sk-{n}")))
    }

    #[test]
    fn set_get_delete_roundtrip() {
        let id = format!("test-{}", std::process::id());
        // 凭据管理器需要真实的交互登录会话：CI 镜像与部分服务账户拿不到，
        // 这时跳过而不是把整个测试套件判失败。
        if let Err(e) = set_api_key(&id, "sk-test-123") {
            eprintln!("跳过：当前环境无法访问系统凭据管理器（{e}）");
            return;
        }
        // 写入 → 读取一致
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
