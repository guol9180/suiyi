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
    /// 服务列表中的排序值（拖拽排序时重排）
    pub order: u32,
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
            order: 0,
        }
    }
}

/// services.json 的根结构，预留全局字段（并发数、超时、回退开关）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ServicesFile {
    pub version: u32,
    pub concurrency: u32,
    pub timeout_secs: u64,
    /// 输入框转译（Alt+T）的目标语言
    pub input_target_lang: String,
    pub services: Vec<ServiceConfig>,
}

/// 输入框转译可选的目标语言，与前端 types.ts 的 TARGET_LANGS 保持一致
pub const INPUT_TARGET_LANGS: [&str; 4] = ["中文", "简体中文", "English", "日本語"];
pub const DEFAULT_INPUT_TARGET_LANG: &str = "English";

impl Default for ServicesFile {
    fn default() -> Self {
        Self {
            version: 1,
            concurrency: 2,
            timeout_secs: 15,
            input_target_lang: DEFAULT_INPUT_TARGET_LANG.into(),
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
        base_url: "https://api.deepseek.com/v1".into(),
        model: "deepseek-chat".into(),
        prompt_template: None,
        temperature: Some(0.3),
        stream: true,
        result_type: ResultType::Text,
        order: 0,
    }]
}

/// 读取 services.json；文件不存在时返回默认内容（不落盘，落盘由调用方决定）
pub fn load_services(dir: &Path) -> Result<ServicesFile, String> {
    let path = dir.join(SERVICES_FILE);
    if !path.exists() {
        return Ok(ServicesFile {
            services: default_services(),
            ..Default::default()
        });
    }
    let raw = fs::read_to_string(&path).map_err(|e| format!("读取配置失败: {e}"))?;
    serde_json::from_str(&raw).map_err(|e| format!("解析配置失败: {e}"))
}

/// 写入 services.json（先写临时文件再改名，避免半写损坏）
pub fn save_services(dir: &Path, file: &ServicesFile) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| format!("创建配置目录失败: {e}"))?;
    let path = dir.join(SERVICES_FILE);
    let tmp = dir.join(format!("{SERVICES_FILE}.tmp"));
    let raw =
        serde_json::to_string_pretty(file).map_err(|e| format!("序列化配置失败: {e}"))?;
    fs::write(&tmp, raw).map_err(|e| format!("写入配置失败: {e}"))?;
    fs::rename(&tmp, &path).map_err(|e| format!("替换配置失败: {e}"))
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
        assert_eq!(reloaded.services[0].base_url, "https://api.deepseek.com/v1");

        // camelCase 字段名落盘（与前端 TS 类型对齐）
        let raw = fs::read_to_string(dir.join(SERVICES_FILE)).unwrap();
        assert!(raw.contains("\"baseUrl\""));
        assert!(raw.contains("\"promptTemplate\""));
        assert!(raw.contains("\"inputTargetLang\""));
        assert!(!raw.to_lowercase().contains("apikey"), "密钥绝不能出现在配置文件");

        let _ = fs::remove_dir_all(&dir);
    }
}
