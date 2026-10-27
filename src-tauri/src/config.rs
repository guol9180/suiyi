//! S0.4 配置层：services.json 的定义、读写与默认值
//!
//! 设计要点：
//! - 所有字段用 camelCase 序列化，与前端 TypeScript 类型一一对应；
//! - API Key 永远不进这份文件，只存系统凭据管理器（见 keyring.rs）；
//! - 读写函数显式接收路径，保持纯净可单测；Tauri 命令层负责提供真实路径。

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

pub const SERVICES_FILE: &str = "services.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceKind {
    Translation,
    Ocr,
    Speech,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Protocol {
    /// OpenAI 兼容 /v1/chat/completions（DeepSeek、智谱、Kimi、Ollama、OneAPI…）
    OpenAiCompatible,
    Anthropic,
    Gemini,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResultType {
    Text,
    Dictionary,
}

/// 一条可配置的服务（对应设计稿「设置 · 服务配置」中的一行/一表单）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ServiceConfig {
    pub id: String,
    pub name: String,
    pub kind: ServiceKind,
    pub protocol: Protocol,
    pub enabled: bool,
    pub base_url: String,
    pub model: String,
    /// Prompt 模板，支持 {{from}} {{to}} {{text}} 变量；None 时用内置默认
    pub prompt_template: Option<String>,
    pub temperature: Option<f32>,
    pub stream: bool,
    pub result_type: ResultType,
    /// 这家服务是否需要 API Key。本地服务（Ollama 之类）填 false，请求就不带 Authorization。
    /// 老配置文件里没有这个字段，缺省按 true —— 不能因为升级就把云端服务的校验放掉。
    #[serde(default = "default_requires_key")]
    pub requires_key: bool,
    /// 服务列表中的排序值（拖拽排序时重排）
    pub order: u32,
    /// 由插件提供的服务填插件 id；普通服务为 None，且这类服务不落盘
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plugin_id: Option<String>,
    /// 思考模式：off=发关闭参数（翻译推荐，首字快且省钱）/ default=不发参数跟随模型
    /// 默认 / on=发开启参数。参数风格按提供商主机名映射，识别不出的提供商不发
    /// （严格网关会拒绝未知字段）。
    pub thinking: String,
}

impl Default for ServiceConfig {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            kind: ServiceKind::Translation,
            protocol: Protocol::OpenAiCompatible,
            enabled: false,
            base_url: String::new(),
            model: String::new(),
            prompt_template: None,
            temperature: Some(0.3),
            stream: true,
            result_type: ResultType::Text,
            requires_key: true,
            order: 0,
            plugin_id: None,
            thinking: "off".to_string(),
        }
    }
}

fn default_requires_key() -> bool {
    true
}

/// services.json 的根结构，预留全局字段（并发数、超时、回退开关）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ServicesFile {
    pub version: u32,
    pub concurrency: u32,
    pub timeout_secs: u64,
    /// AnkiConnect 地址与目标牌组
    pub anki_url: String,
    pub anki_deck: String,
    /// 朗读用的系统音色 id 与语速倍数。空 id 表示跟随系统默认音色。
    #[serde(default)]
    pub speech_voice: String,
    #[serde(default = "default_speech_rate")]
    pub speech_rate: f64,
    /// 点窗口 × 时怎么办：ask（每次问）/ tray（收进右下角托盘）/ quit（直接退出）
    #[serde(default = "default_close_action")]
    pub close_action: String,
    /// 启动后自动检查一次更新
    #[serde(default = "default_true")]
    pub auto_check_update: bool,
    /// 用户改过的全局热键，键是动作 id（selection / screenshot），
    /// 值是 "alt+d" 这种加速度字符串。没写过的动作走默认值。
    /// 老配置里可能还留着已移除的 input，读进来不报错、也不会被注册。
    #[serde(default)]
    pub hotkeys: std::collections::BTreeMap<String, String>,
    pub services: Vec<ServiceConfig>,
}

/// 合法的关闭行为。写进配置前一律过一遍这张表，脏数据不进文件。
pub const CLOSE_ACTIONS: [&str; 3] = ["ask", "tray", "quit"];

pub fn default_close_action() -> String {
    DEFAULT_CLOSE_ACTION.to_string()
}

/// 出厂关闭行为：每次询问。收窗口等于退出热键可用性，不能替用户默认掉。
pub const DEFAULT_CLOSE_ACTION: &str = "ask";

/// 把配置里读到的关闭行为收敛到合法值。
///
/// v0.8.1 把「最小化到任务栏」换成了「收进托盘」，老配置里的 minimize 按 tray 处理：
/// 用户当初选的意思是「别退出、收起来」，托盘正是这个意思的落点。
pub fn normalize_close_action(raw: &str) -> String {
    match raw.trim() {
        "minimize" | "tray" => "tray".to_string(),
        "quit" => "quit".to_string(),
        _ => DEFAULT_CLOSE_ACTION.to_string(),
    }
}

pub fn default_true() -> bool {
    true
}

/// 老配置文件里没有 speech_rate，缺省按原速
fn default_speech_rate() -> f64 {
    crate::speech::DEFAULT_RATE
}

impl Default for ServicesFile {
    fn default() -> Self {
        Self {
            version: 1,
            concurrency: 2,
            timeout_secs: 15,
            anki_url: crate::anki::DEFAULT_ANKI_URL.into(),
            anki_deck: crate::anki::DEFAULT_ANKI_DECK.into(),
            speech_voice: String::new(),
            speech_rate: crate::speech::DEFAULT_RATE,
            close_action: default_close_action(),
            auto_check_update: true,
            hotkeys: std::collections::BTreeMap::new(),
            services: Vec::new(),
        }
    }
}

/// 默认 Prompt 模板（设计稿④中的模板一致）
pub const DEFAULT_PROMPT: &str =
    "你是专业翻译引擎。将{{from}}翻译为{{to}}，只输出译文：\n{{text}}";

/// 词典结构化的默认 Prompt：要求模型只吐严格 JSON，方便解析。
/// 服务若自带 prompt_template，则尊重用户的模板，解析失败会回退为纯文本。
pub const DICTIONARY_PROMPT: &str = "你是词典引擎。把{{text}}（{{from}} → {{to}}）整理成词条，\
只输出严格 JSON，不要代码块、不要任何解释：\n\
{\"word\":\"词条原形\",\"phonetic\":\"音标，没有就留空\",\
\"senses\":[{\"pos\":\"词性缩写\",\"def\":\"释义\",\"example\":\"例句，可空\"}]}";

/// 首次启动时的预置服务（全部未启用、无密钥，用户在设置页填 Key）
pub fn default_services() -> Vec<ServiceConfig> {
    vec![ServiceConfig {
        id: "preset-deepseek".into(),
        name: "DeepSeek".into(),
        kind: ServiceKind::Translation,
        protocol: Protocol::OpenAiCompatible,
        enabled: false,
        // 官方文档：Base URL 不带 /v1，模型名是 deepseek-flash（旧名 deepseek-chat 已下线）
        base_url: "https://api.deepseek.com".into(),
        model: "deepseek-flash".into(),
        prompt_template: None,
        temperature: Some(0.3),
        stream: true,
        result_type: ResultType::Text,
        requires_key: true,
        order: 0,
        plugin_id: None,
        thinking: "off".into(),
    }]
}

/// services.json 的进程内缓存槽位：翻译每次都要读一遍服务配置，读多写少，
/// 按「文件是否存在 + 修改时间」判定命中，命中就不碰磁盘。
/// 外部手改文件会改变 mtime，下一次读取自然失效，不需要额外协议。
type ServicesCacheSlot = Option<(bool, Option<std::time::SystemTime>, ServicesFile)>;

static SERVICES_CACHE: std::sync::OnceLock<std::sync::Mutex<ServicesCacheSlot>> =
    std::sync::OnceLock::new();

fn services_cache() -> &'static std::sync::Mutex<ServicesCacheSlot> {
    SERVICES_CACHE.get_or_init(|| std::sync::Mutex::new(None))
}

/// 命中判定：签名完全一致才返回缓存内容
fn cache_get(
    slot: &ServicesCacheSlot,
    exists: bool,
    modified: Option<std::time::SystemTime>,
) -> Option<ServicesFile> {
    slot.as_ref()
        .filter(|(e, m, _)| *e == exists && *m == modified)
        .map(|(_, _, f)| f.clone())
}

/// 写入缓存槽位
fn cache_put(
    slot: &mut ServicesCacheSlot,
    exists: bool,
    modified: Option<std::time::SystemTime>,
    file: &ServicesFile,
) {
    *slot = Some((exists, modified, file.clone()));
}

/// 读取 services.json；文件不存在时返回默认内容（不落盘，落盘由调用方决定）。
/// 命中进程内缓存时直接返回副本，不再读磁盘。
pub fn load_services(dir: &Path) -> Result<ServicesFile, String> {
    let path = dir.join(SERVICES_FILE);
    let meta = fs::metadata(&path).ok();
    let exists = meta.is_some();
    let modified = meta.and_then(|m| m.modified().ok());
    if let Ok(guard) = services_cache().lock() {
        if let Some(cached) = cache_get(&guard, exists, modified) {
            return Ok(cached);
        }
    }
    let file = if !exists {
        ServicesFile {
            services: default_services(),
            ..Default::default()
        }
    } else {
        let raw = fs::read_to_string(&path).map_err(|e| format!("读取配置失败: {e}"))?;
        serde_json::from_str(&raw).map_err(|e| format!("解析配置失败: {e}"))?
    };
    if let Ok(mut guard) = services_cache().lock() {
        cache_put(&mut guard, exists, modified, &file);
    }
    Ok(file)
}

/// 写入 services.json（先写临时文件再改名，避免半写损坏）。
/// 新内容与新的修改时间写穿进缓存，下一次读取不用再碰磁盘。
pub fn save_services(dir: &Path, file: &ServicesFile) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| format!("创建配置目录失败: {e}"))?;
    let path = dir.join(SERVICES_FILE);
    let tmp = dir.join(format!("{SERVICES_FILE}.tmp"));
    let raw =
        serde_json::to_string_pretty(file).map_err(|e| format!("序列化配置失败: {e}"))?;
    fs::write(&tmp, raw).map_err(|e| format!("写入配置失败: {e}"))?;
    fs::rename(&tmp, &path).map_err(|e| format!("替换配置失败: {e}"))?;
    let modified = fs::metadata(&path).and_then(|m| m.modified()).ok();
    if let Ok(mut guard) = services_cache().lock() {
        cache_put(&mut guard, true, modified, file);
    }
    Ok(())
}

/// 生成短 ID（时间戳 + 计数足够单机使用；不引额外依赖）
pub fn new_service_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("svc-{ts}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 缓存命中判定：文件存在性与修改时间都一致才命中，任一变化即失效。
    /// 这是「读服务配置不再每次碰磁盘」的核心逻辑。
    #[test]
    fn services_cache_hits_only_on_matching_signature() {
        let mut slot: Option<(bool, Option<std::time::SystemTime>, ServicesFile)> = None;
        assert!(cache_get(&slot, true, None).is_none(), "空缓存不命中");

        let f = ServicesFile::default();
        cache_put(&mut slot, true, None, &f);
        assert!(cache_get(&slot, true, None).is_some(), "同签名命中");

        let t = std::time::SystemTime::now();
        cache_put(&mut slot, true, Some(t), &f);
        assert!(cache_get(&slot, true, Some(t)).is_some(), "同签名命中");
        assert!(cache_get(&slot, true, None).is_none(), "mtime 变化要失效");
        assert!(cache_get(&slot, false, Some(t)).is_none(), "文件被删要失效");
    }

    /// 进程内缓存必须与磁盘保持一致：save 之后再 load 拿到新内容（写穿），
    /// 外部绕过 save 直接改文件（mtime 变化）也要能让缓存失效。
    /// 翻译每次都要读服务配置，缓存命中就不再碰磁盘。
    #[test]
    fn services_cache_stays_consistent_with_disk() {
        let dir = std::env::temp_dir().join(format!("suiyi-cfg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let a = ServicesFile::default();
        save_services(&dir, &a).unwrap();
        assert!(load_services(&dir).unwrap().services.is_empty());

        // 保存新内容后立刻读取：写穿缓存不允许读到旧值
        let mut b = a.clone();
        b.concurrency = 5;
        save_services(&dir, &b).unwrap();
        assert_eq!(load_services(&dir).unwrap().concurrency, 5);

        // 模拟外部手改：绕过 save_services 直接写文件，mtime 变化 → 缓存必须失效
        std::thread::sleep(std::time::Duration::from_millis(60));
        let path = dir.join(SERVICES_FILE);
        let mut c: ServicesFile =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        c.timeout_secs = 42;
        std::fs::write(&path, serde_json::to_string_pretty(&c).unwrap()).unwrap();
        assert_eq!(load_services(&dir).unwrap().timeout_secs, 42, "外部改动要能被读到");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 老配置文件里没有 requires_key：必须按 true 处理。
    /// 升级不能把云端服务的密钥校验顺手放掉。
    #[test]
    fn requires_key_defaults_to_true_for_old_config() {
        let old = r#"{
          "version": 1,
          "services": [
            {"id":"a","name":"A","kind":"translation","protocol":"open_ai_compatible",
             "enabled":true,"baseUrl":"https://example.com","model":"m"}
          ]
        }"#;
        let file: ServicesFile = serde_json::from_str(old).unwrap();
        assert!(file.services[0].requires_key, "缺字段时应当需要密钥");

        // 回写时字段要落盘，下次读还是 true
        let json = serde_json::to_string(&file.services[0]).unwrap();
        assert!(json.contains("\"requiresKey\":true"), "实际：{json}");
    }

    /// 本地服务（Ollama 这类）显式写 false 时能被读回
    #[test]
    fn requires_key_false_survives_roundtrip() {
        let raw = r#"{"id":"local","name":"Ollama","kind":"translation",
          "protocol":"open_ai_compatible","enabled":true,
          "baseUrl":"http://localhost:11434/v1","model":"qwen3:8b","requiresKey":false}"#;
        let svc: ServiceConfig = serde_json::from_str(raw).unwrap();
        assert!(!svc.requires_key);
        assert_eq!(svc.model, "qwen3:8b");
    }

    /// 老配置没有 closeAction / autoCheckUpdate：缺省必须是「每次询问」与「自动检查」
    #[test]
    fn close_action_defaults_to_ask() {
        let old = r#"{"version":1,"services":[]}"#;
        let file: ServicesFile = serde_json::from_str(old).unwrap();
        assert_eq!(file.close_action, "ask");
        assert!(file.auto_check_update);

        let mut f = ServicesFile::default();
        f.close_action = "tray".into();
        let json = serde_json::to_string(&f).unwrap();
        assert!(json.contains("\"closeAction\":\"tray\""), "实际：{json}");
        assert!(json.contains("\"autoCheckUpdate\":true"), "实际：{json}");
    }

    /// 老配置里的 minimize（最小化到任务栏）统一按「收进托盘」处理，脏值回落到每次询问
    #[test]
    fn close_action_normalizes_legacy_values() {
        assert_eq!(normalize_close_action("minimize"), "tray");
        assert_eq!(normalize_close_action(" tray "), "tray");
        assert_eq!(normalize_close_action("quit"), "quit");
        assert_eq!(normalize_close_action("ask"), "ask");
        assert_eq!(normalize_close_action(""), "ask");
        assert_eq!(normalize_close_action("whatever"), "ask");
    }

    #[test]
    fn roundtrip_and_defaults() {
        let dir = std::env::temp_dir().join(format!("suiyi-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);

        // 不存在 → 默认（含预置 DeepSeek，未启用）
        let loaded = load_services(&dir).unwrap();
        assert_eq!(loaded.services.len(), 1);
        assert_eq!(loaded.services[0].id, "preset-deepseek");
        assert!(!loaded.services[0].enabled);

        // 修改 → 保存 → 再读，内容一致
        let mut file = loaded;
        file.services[0].enabled = true;
        save_services(&dir, &file).unwrap();
        let reloaded = load_services(&dir).unwrap();
        assert!(reloaded.services[0].enabled);
        // 预置值按官方文档：Base URL 不带 /v1，模型是 deepseek-flash
        assert_eq!(reloaded.services[0].base_url, "https://api.deepseek.com");
        assert_eq!(reloaded.services[0].model, "deepseek-flash");
        assert!(reloaded.services[0].requires_key);

        // camelCase 字段名落盘（与前端 TS 类型对齐）
        let raw = fs::read_to_string(dir.join(SERVICES_FILE)).unwrap();
        assert!(raw.contains("\"baseUrl\""));
        assert!(raw.contains("\"promptTemplate\""));
        assert!(raw.contains("\"closeAction\""));
        assert!(!raw.to_lowercase().contains("apikey"), "密钥绝不能出现在配置文件");

        let _ = fs::remove_dir_all(&dir);
    }
}
