//! 全局热键的注册与状态上报。
//!
//! 插件自带的 `with_shortcuts` 是「全成或全不成」：它在插件 setup 里循环注册，
//! 只要有一个被别的程序占用就整个返回错误，连插件状态都不会建立。
//! 后果是一个热键冲突会让另外两个一起失效，而且界面上看不到任何原因。
//!
//! 这里改成逐个注册，把每个热键的结果留下来交给界面显示。

use serde::Serialize;
use std::sync::Mutex;
use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};

/// 热键定义。与 lib.rs 里的事件比对、界面上的三行一一对应。
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
}

impl HotkeyStatus {
    fn new(
        id: &str,
        label: &str,
        accelerator: &str,
        registered: bool,
        error: Option<String>,
    ) -> Self {
        Self {
            id: id.to_string(),
            label: label.to_string(),
            accelerator: accelerator.to_string(),
            registered,
            error,
        }
    }
}

/// 最近一次注册结果，命令层读它来回答界面
#[derive(Default)]
pub struct HotkeyState(pub Mutex<Vec<HotkeyStatus>>);

impl HotkeyState {
    pub fn snapshot(&self) -> Vec<HotkeyStatus> {
        self.0.lock().map(|v| v.clone()).unwrap_or_default()
    }

    pub fn store(&self, list: Vec<HotkeyStatus>) {
        if let Ok(mut slot) = self.0.lock() {
            *slot = list;
        }
    }
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

/// 逐个注册，返回每个热键的结果。已经被本进程注册过的直接算成功。
pub fn register_all(app: &AppHandle) -> Vec<HotkeyStatus> {
    let gs = app.global_shortcut();
    ENTRIES
        .iter()
        .map(|(id, label, accel)| {
            let shortcut: Shortcut = match accel.parse() {
                Ok(s) => s,
                Err(e) => {
                    return HotkeyStatus::new(
                        id,
                        label,
                        accel,
                        false,
                        Some(format!("无法解析 {accel}：{e}")),
                    )
                }
            };
            if gs.is_registered(shortcut.clone()) {
                return HotkeyStatus::new(id, label, accel, true, None);
            }
            match gs.register(shortcut) {
                Ok(_) => HotkeyStatus::new(id, label, accel, true, None),
                Err(e) => {
                    let reason = explain(&e.to_string());
                    crate::selection::log_line(&format!("hotkey: {accel} 注册失败: {e}"));
                    HotkeyStatus::new(id, label, accel, false, Some(reason))
                }
            }
        })
        .collect()
}

/// 补注册：只重试还没成功的那些，直到全部成功或试满 `rounds` 轮。
/// 启动瞬间旧实例可能还没释放热键，所以需要重试。
pub fn retry_until_ready(app: &AppHandle, rounds: u32, interval: std::time::Duration) {
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
        let list = register_all(app);
        let still = list.iter().filter(|s| !s.registered).count();
        app.state::<HotkeyState>().store(list);
        crate::selection::log_line(&format!("hotkey: 补注册一轮，仍未注册 {still} 个"));
    }
}
