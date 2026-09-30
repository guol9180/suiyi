//! M5 插件系统：清单解析、发现与校验。
//!
//! 目录约定：`%APPDATA%/com.suiyi.dev/plugins/<插件目录>/manifest.json`，
//! 入口脚本相对清单所在目录解析，**不允许逃出插件目录**（路径穿越防护）。
//!
//! 设计取舍：
//! - 校验失败的插件也列出来并带上原因，直接「消失」比报错更难排查；
//! - 未知的 kind / 权限一律拒绝而不是忽略，避免插件以为自己拿到了没拿到的能力。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const PLUGINS_DIR: &str = "plugins";
pub const MANIFEST_FILE: &str = "manifest.json";
pub const STATE_FILE: &str = "plugins.json";

/// 插件类型，对应四类扩展点
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginKind {
    /// 翻译：可作为服务出现在服务列表里
    Translation,
    /// OCR：把图片识别成文字
    Ocr,
    /// 语音：朗读文本
    Speech,
    /// 动作：对译文做自定义处理（复制、替换、上报等）
    Action,
}

impl PluginKind {
    fn parse(s: &str) -> Option<Self> {
        match s {
            "translation" => Some(Self::Translation),
            "ocr" => Some(Self::Ocr),
            "speech" => Some(Self::Speech),
            "action" => Some(Self::Action),
            _ => None,
        }
    }
}

/// 已知权限。插件只允许申请这里列出的能力。
pub const KNOWN_PERMISSIONS: [&str; 2] = ["http", "clipboard"];

#[derive(Debug, Clone, Deserialize)]
struct Manifest {
    id: String,
    name: String,
    #[serde(default)]
    version: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    author: String,
    main: String,
    kind: String,
    #[serde(default)]
    permissions: Vec<String>,
}

/// 一个插件的校验结果。`ok=false` 时 `error` 说明原因，插件仍会出现在列表里。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: String,
    pub kind: Option<PluginKind>,
    pub permissions: Vec<String>,
    /// 插件目录绝对路径
    pub dir: String,
    /// 入口脚本绝对路径（校验通过时才有）
    pub main: Option<String>,
    pub enabled: bool,
    pub ok: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PluginState {
    /// 被用户停用的插件 id
    pub disabled: Vec<String>,
}

pub fn plugins_dir(config_dir: &Path) -> PathBuf {
    config_dir.join(PLUGINS_DIR)
}

fn load_state(config_dir: &Path) -> PluginState {
    let path = config_dir.join(STATE_FILE);
    std::fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn save_state(config_dir: &Path, state: &PluginState) -> Result<(), String> {
    std::fs::create_dir_all(config_dir).map_err(|e| format!("创建配置目录失败: {e}"))?;
    let path = config_dir.join(STATE_FILE);
    let raw = serde_json::to_string_pretty(state).map_err(|e| format!("序列化插件状态失败: {e}"))?;
    std::fs::write(&path, raw).map_err(|e| format!("写入插件状态失败: {e}"))
}

/// 校验单个插件目录
fn load_one(dir: &Path, disabled: &[String]) -> PluginInfo {
    let fallback_id = dir
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let mut info = PluginInfo {
        id: fallback_id.clone(),
        name: fallback_id,
        version: String::new(),
        description: String::new(),
        author: String::new(),
        kind: None,
        permissions: Vec::new(),
        dir: dir.to_string_lossy().to_string(),
        main: None,
        enabled: true,
        ok: false,
        error: None,
    };

    let manifest_path = dir.join(MANIFEST_FILE);
    let raw = match std::fs::read_to_string(&manifest_path) {
        Ok(r) => r,
        Err(e) => {
            info.error = Some(format!("读不到 {MANIFEST_FILE}: {e}"));
            return info;
        }
    };
    let manifest: Manifest = match serde_json::from_str(&raw) {
        Ok(m) => m,
        Err(e) => {
            info.error = Some(format!("清单格式不对: {e}"));
            return info;
        }
    };

    info.id = manifest.id.trim().to_string();
    info.name = manifest.name.trim().to_string();
    info.version = manifest.version.trim().to_string();
    info.description = manifest.description.trim().to_string();
    info.author = manifest.author.trim().to_string();
    info.enabled = !disabled.iter().any(|d| d == &info.id);

    if info.id.is_empty() {
        info.error = Some("id 不能为空".into());
        return info;
    }
    if info.name.is_empty() {
        info.error = Some("name 不能为空".into());
        return info;
    }
    let Some(kind) = PluginKind::parse(manifest.kind.trim()) else {
        info.error = Some(format!(
            "未知的 kind「{}」，只能是 translation / ocr / speech / action",
            manifest.kind
        ));
        return info;
    };
    info.kind = Some(kind);

    // 权限只认白名单里的，出现未知项直接拒绝，避免插件以为自己拿到了别的东西
    let mut permissions = Vec::new();
    for p in &manifest.permissions {
        let p = p.trim();
        if p.is_empty() {
            continue;
        }
        if !KNOWN_PERMISSIONS.contains(&p) {
            info.error = Some(format!("未知权限「{p}」，目前只支持 http / clipboard"));
            return info;
        }
        permissions.push(p.to_string());
    }
    info.permissions = permissions;

    // 入口脚本必须落在插件目录内：防住 ../../ 这类路径穿越
    let main = dir.join(manifest.main.trim());
    let (Ok(dir_real), Ok(main_real)) = (dir.canonicalize(), main.canonicalize()) else {
        info.error = Some(format!("入口脚本不存在: {}", manifest.main.trim()));
        return info;
    };
    if !main_real.starts_with(&dir_real) {
        info.error = Some(format!("入口脚本越出了插件目录: {}", manifest.main.trim()));
        return info;
    }
    if !main_real.is_file() {
        info.error = Some("入口脚本不是文件".into());
        return info;
    }
    info.main = Some(main_real.to_string_lossy().to_string());
    info.ok = true;
    info
}

/// 扫描插件目录。目录不存在时返回空列表（首次使用属正常情况）。
pub fn discover(config_dir: &Path) -> Vec<PluginInfo> {
    let root = plugins_dir(config_dir);
    let disabled = load_state(config_dir).disabled;
    let Ok(entries) = std::fs::read_dir(&root) else {
        return Vec::new();
    };
    let mut list: Vec<PluginInfo> = entries
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| load_one(&e.path(), &disabled))
        .collect();
    list.sort_by(|a, b| a.name.cmp(&b.name));
    list
}

/// 某个扩展点下所有可用（校验通过且启用）的插件
pub fn enabled_of(config_dir: &Path, kind: PluginKind) -> Vec<PluginInfo> {
    discover(config_dir)
        .into_iter()
        .filter(|p| p.ok && p.enabled && p.kind == Some(kind))
        .collect()
}

/// 取第一个可用的插件。用于「装了就用插件、没装用内置实现」这类替换式扩展点。
pub fn first_enabled(config_dir: &Path, kind: PluginKind) -> Option<PluginInfo> {
    enabled_of(config_dir, kind).into_iter().next()
}

/// 启用/停用插件
pub fn set_enabled(config_dir: &Path, id: &str, enabled: bool) -> Result<(), String> {
    let mut state = load_state(config_dir);
    state.disabled.retain(|d| d != id);
    if !enabled {
        state.disabled.push(id.to_string());
    }
    save_state(config_dir, &state)
}

/// 首次使用时铺一个示例插件，省得用户对着空目录猜格式
pub fn ensure_sample(config_dir: &Path) -> Result<PathBuf, String> {
    let dir = plugins_dir(config_dir).join("sample-uppercase");
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建示例插件目录失败: {e}"))?;

    let manifest = serde_json::json!({
        "id": "com.suiyi.sample.uppercase",
        "name": "示例 · 转大写",
        "version": "1.0.0",
        "description": "演示用插件：把译文整体转成大写。复制这个目录就能开始写自己的插件。",
        "author": "随译",
        "main": "index.js",
        "kind": "translation",
        "permissions": []
    });
    std::fs::write(
        dir.join(MANIFEST_FILE),
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .map_err(|e| format!("写入示例清单失败: {e}"))?;

    let script = r#"// 随译插件示例：translate 接收 { text, from, to }，返回译文
function translate(input) {
  host.log("示例插件收到 " + input.text.length + " 个字符");
  return "[" + input.to + "] " + input.text.toUpperCase();
}
"#;
    std::fs::write(dir.join("index.js"), script).map_err(|e| format!("写入示例脚本失败: {e}"))?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("suiyi-plugin-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn write_plugin(root: &Path, name: &str, manifest: serde_json::Value, script: &str) -> PathBuf {
        let dir = plugins_dir(root).join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(MANIFEST_FILE),
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();
        if !script.is_empty() {
            std::fs::write(dir.join("index.js"), script).unwrap();
        }
        dir
    }

    fn manifest(id: &str, main: &str, kind: &str, perms: serde_json::Value) -> serde_json::Value {
        serde_json::json!({
            "id": id, "name": id, "version": "1.0.0", "main": main,
            "kind": kind, "permissions": perms
        })
    }

    #[test]
    fn discovers_valid_plugin() {
        let root = tmp("valid");
        write_plugin(&root, "demo", manifest("com.x.demo", "index.js", "translation", serde_json::json!([])), "function translate(i){return i.text;}");

        let list = discover(&root);
        assert_eq!(list.len(), 1);
        let p = &list[0];
        assert!(p.ok, "应通过校验: {:?}", p.error);
        assert_eq!(p.id, "com.x.demo");
        assert_eq!(p.kind, Some(PluginKind::Translation));
        assert!(p.enabled);
        assert!(p.main.as_ref().unwrap().ends_with("index.js"));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn rejects_path_traversal_and_unknown_fields() {
        let root = tmp("reject");
        // 入口脚本指向插件目录之外。目标文件确实存在，这样才能真正验到路径穿越那道闸。
        std::fs::create_dir_all(plugins_dir(&root)).unwrap();
        std::fs::write(plugins_dir(&root).join("outside.js"), "function translate(){}").unwrap();
        write_plugin(&root, "escape", manifest("com.x.escape", "../outside.js", "translation", serde_json::json!([])), "");

        // 未知 kind
        write_plugin(&root, "badkind", manifest("com.x.badkind", "index.js", "hacking", serde_json::json!([])), "x");
        // 未知权限
        write_plugin(&root, "badperm", manifest("com.x.badperm", "index.js", "ocr", serde_json::json!(["filesystem"])), "x");
        // 清单缺失
        std::fs::create_dir_all(plugins_dir(&root).join("empty")).unwrap();

        let list = discover(&root);
        assert_eq!(list.len(), 4);
        assert!(list.iter().all(|p| !p.ok), "四个都不该通过校验");
        let by_id = |id: &str| list.iter().find(|p| p.id == id).unwrap();
        assert!(by_id("com.x.escape").error.as_deref().unwrap().contains("越出"));
        assert!(by_id("com.x.badkind").error.as_deref().unwrap().contains("未知的 kind"));
        assert!(by_id("com.x.badperm").error.as_deref().unwrap().contains("未知权限"));
        assert!(by_id("empty").error.as_deref().unwrap().contains("读不到"));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn enable_state_round_trips() {
        let root = tmp("state");
        write_plugin(&root, "demo", manifest("com.x.demo", "index.js", "action", serde_json::json!(["clipboard"])), "x");

        assert!(discover(&root)[0].enabled);
        set_enabled(&root, "com.x.demo", false).unwrap();
        let after = discover(&root);
        assert!(!after[0].enabled);
        assert_eq!(after[0].permissions, vec!["clipboard"]);
        set_enabled(&root, "com.x.demo", true).unwrap();
        assert!(discover(&root)[0].enabled);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn sample_plugin_is_valid() {
        let root = tmp("sample");
        let dir = ensure_sample(&root).unwrap();
        assert!(dir.join("index.js").exists());
        let list = discover(&root);
        assert_eq!(list.len(), 1);
        assert!(list[0].ok, "示例插件必须能通过自己的校验: {:?}", list[0].error);

        let _ = std::fs::remove_dir_all(&root);
    }
}
