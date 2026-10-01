//! 全局热键的注册、改键与状态上报。
//!
//! 插件自带的 `with_shortcuts` 是「全成或全不成」：它在插件 setup 里循环注册，
//! 只要有一个被别的程序占用就整个返回错误，连插件状态都不会建立。
//! 后果是一个热键冲突会让另外两个一起失效，而且界面上看不到任何原因。
//!
//! 这里改成逐个注册，把每个热键的结果留下来交给界面显示；
//! 事件回调也不再比对写死的快捷键，而是查当前生效的那张表，这样才支持改键。

use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::Mutex;
use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};

/// 三个动作与它们的出厂热键
pub const ENTRIES: [(&str, &str, &str); 3] = [
    ("selection", "划词翻译", "alt+d"),
    ("screenshot", "截图识别", "alt+s"),
    ("input", "输入框转译", "alt+t"),
];

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyStatus {
    pub id: String,
    pub label: String,
    pub accelerator: String,
    pub registered: bool,
    /// 注册失败的原因，成功时为 null
    pub error: Option<String>,
    /// 是否已被用户改过（界面据此显示「恢复默认」）
    pub custom: bool,
}

impl HotkeyStatus {
    fn new(id: &str, label: &str, accel: &str, registered: bool, error: Option<String>, custom: bool) -> Self {
        Self {
            id: id.to_string(),
            label: label.to_string(),
            accelerator: accel.to_string(),
            registered,
            error,
            custom,
        }
    }
}

/// 最近一次注册结果 + 当前生效的 id→快捷键表
#[derive(Default)]
pub struct HotkeyState {
    status: Mutex<Vec<HotkeyStatus>>,
    active: Mutex<Vec<(String, Shortcut)>>,
}

impl HotkeyState {
    pub fn snapshot(&self) -> Vec<HotkeyStatus> {
        self.status.lock().map(|v| v.clone()).unwrap_or_default()
    }

    pub fn store(&self, list: Vec<HotkeyStatus>) {
        let pairs: Vec<(String, Shortcut)> = list
            .iter()
            .filter(|s| s.registered)
            .filter_map(|s| {
                s.accelerator
                    .parse::<Shortcut>()
                    .ok()
                    .map(|sc| (s.id.clone(), sc))
            })
            .collect();
        if let Ok(mut slot) = self.active.lock() {
            *slot = pairs;
        }
        if let Ok(mut slot) = self.status.lock() {
            *slot = list;
        }
    }

    /// 事件回调用：这个快捷键当前对应哪个动作
    pub fn action_of(&self, pressed: &Shortcut) -> Option<String> {
        let active = self.active.lock().ok()?;
        active
            .iter()
            .find(|(_, sc)| sc == pressed)
            .map(|(id, _)| id.clone())
    }
}

/// 出厂热键，改键失败时用来回落
pub fn default_of(id: &str) -> &'static str {
    ENTRIES
        .iter()
        .find(|(eid, _, _)| *eid == id)
        .map(|(_, _, d)| *d)
        .unwrap_or("alt+d")
}

/// 把配置里的自定义热键与出厂值合起来，得到当前该注册的列表
pub fn effective(config: &BTreeMap<String, String>) -> Vec<(String, String)> {
    ENTRIES
        .iter()
        .map(|(id, _, def)| {
            let accel = config
                .get(*id)
                .filter(|v| !v.trim().is_empty())
                .cloned()
                .unwrap_or_else(|| (*def).to_string());
            ((*id).to_string(), accel)
        })
        .collect()
}

/// 把系统返回的原始错误翻译成用户能读懂的一句话
fn explain(raw: &str) -> String {
    let lower = raw.to_lowercase();
    if lower.contains("already registered") || raw.contains("已注册") || raw.contains("占用") {
        "该组合已被其他程序占用".to_string()
    } else {
        raw.to_string()
    }
}

/// 先卸掉旧的再逐个注册。改键后调用它，避免旧键留下残影。
pub fn apply(app: &AppHandle, pairs: &[(String, String)]) -> Vec<HotkeyStatus> {
    let gs = app.global_shortcut();
    if let Ok(active) = app.state::<HotkeyState>().active.lock() {
        for (_, sc) in active.iter() {
            let _ = gs.unregister(sc.clone());
        }
    }
    if let Ok(mut slot) = app.state::<HotkeyState>().active.lock() {
        slot.clear();
    }

    pairs
        .iter()
        .map(|(id, accel)| {
            let label = ENTRIES
                .iter()
                .find(|(eid, _, _)| eid == id)
                .map(|(_, l, _)| *l)
                .unwrap_or(id.as_str());
            let custom = accel != default_of(id);
            let shortcut: Shortcut = match accel.parse() {
                Ok(s) => s,
                Err(e) => {
                    return HotkeyStatus::new(
                        id,
                        label,
                        accel,
                        false,
                        Some(format!("无法解析 {accel}：{e}")),
                        custom,
                    )
                }
            };
            match gs.register(shortcut) {
                Ok(_) => HotkeyStatus::new(id, label, accel, true, None, custom),
                Err(e) => {
                    crate::selection::log_line(&format!("hotkey: {accel} 注册失败: {e}"));
                    HotkeyStatus::new(id, label, accel, false, Some(explain(&e.to_string())), custom)
                }
            }
        })
        .collect()
}

/// 补注册：只重试还没成功的那些，直到全部成功或试满 `rounds` 轮。
/// 启动瞬间旧实例可能还没释放热键，所以需要重试。
pub fn retry_until_ready(app: &AppHandle, pairs: &[(String, String)], rounds: u32, interval: std::time::Duration) {
    for _ in 0..rounds {
        let pending = app
            .state::<HotkeyState>()
            .snapshot()
            .into_iter()
            .filter(|s| !s.registered)
            .count();
        if pending == 0 {
            return;
        }
        std::thread::sleep(interval);
        let list = apply(app, pairs);
        let still = list.iter().filter(|s| !s.registered).count();
        app.state::<HotkeyState>().store(list);
        crate::selection::log_line(&format!("hotkey: 补注册一轮，仍未注册 {still} 个"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 没有自定义时全部走出厂值() {
        let pairs = effective(&BTreeMap::new());
        assert_eq!(pairs.len(), 3);
        assert_eq!(pairs[0], ("selection".to_string(), "alt+d".to_string()));
        assert_eq!(pairs[1], ("screenshot".to_string(), "alt+s".to_string()));
        assert_eq!(pairs[2], ("input".to_string(), "alt+t".to_string()));
    }

    #[test]
    fn 自定义值覆盖默认且空值被忽略() {
        let mut cfg = BTreeMap::new();
        cfg.insert("selection".to_string(), "ctrl+shift+d".to_string());
        cfg.insert("screenshot".to_string(), "   ".to_string());
        let pairs = effective(&cfg);
        assert_eq!(pairs[0].1, "ctrl+shift+d");
        assert_eq!(pairs[1].1, "alt+s", "空白自定义值应当退回默认");
        assert_eq!(pairs[2].1, "alt+t");
    }

    #[test]
    fn 默认值查表能兜住未知动作() {
        assert_eq!(default_of("input"), "alt+t");
        assert_eq!(default_of("nope"), "alt+d");
    }
}
