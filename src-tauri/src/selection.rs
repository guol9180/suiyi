//! M1 划词取词：全局热键触发 → 剪贴板法抓取选中文本 → 光标处弹出翻译窗
//!
//! 策略（与 Pot 相同的兜底方案）：
//! 1. 记录当前剪贴板内容并清空；
//! 2. 模拟 Ctrl+C 让目标应用把选中文本写入剪贴板；
//! 3. 读回剪贴板：非空且与原内容不同 → 视为选中文本；
//! 4. 恢复原剪贴板（不打扰用户），并记下目标窗口句柄供「替换原文」使用。
//! UIA 直读是后续增强，不阻塞本里程碑。

use std::sync::atomic::{AtomicIsize, Ordering};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

const POPUP_W: f64 = 430.0;
const POPUP_H: f64 = 540.0;

/// 最近一次取词时目标窗口的句柄。弹窗里的「替换原文」要靠它把焦点送回去，
/// 否则粘贴会落到别的地方。
static LAST_TARGET: AtomicIsize = AtomicIsize::new(0);

pub(crate) fn last_target() -> isize {
    LAST_TARGET.load(Ordering::SeqCst)
}

#[cfg(windows)]
pub(crate) fn foreground_hwnd() -> isize {
    use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
    unsafe { GetForegroundWindow().0 as isize }
}

/// 追加一行调试日志到 %APPDATA%/com.suiyi.dev/debug.log（诊断热键链路用）
pub fn log_line(msg: &str) {
    let Some(dir) = std::env::var_os("APPDATA").map(std::path::PathBuf::from) else {
        return;
    };
    let dir = dir.join("com.suiyi.dev");
    let _ = std::fs::create_dir_all(&dir);
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("debug.log"))
    {
        use std::io::Write;
        let _ = writeln!(f, "[{ts}] {msg}");
    }
}

/// 热键入口：抓取选中文本并弹出翻译窗（重活放独立线程，不阻塞热键处理）
pub fn trigger_selection_translate(app: AppHandle) {
    log_line("trigger: Alt+D 热键触发");
    std::thread::spawn(move || {
        let text = match capture_selection() {
            Some(t) => {
                log_line(&format!("capture: 抓取到选中文本 {} 字符", t.chars().count()));
                t
            }
            None => {
                log_line("capture: 未取到选中文本（剪贴板无变化或为空），流程结束");
                return;
            }
        };
        if let Err(e) = ensure_popup_at_cursor(&app) {
            log_line(&format!("popup: 弹窗创建/定位失败: {e}"));
            return;
        }
        // 等弹窗 webview 完成挂载监听
        std::thread::sleep(Duration::from_millis(150));
        let _ = app.emit(
            "popup-set-source",
            serde_json::json!({ "text": text, "autoTranslate": true, "kind": "selection" }),
        );
        log_line("emit: 已投递文本到弹窗");
    });
}

/// 模拟 Ctrl + 某个虚拟键。用 SendInput 发送真实虚拟键码，
/// Unicode 注入（KEYEVENTF_UNICODE）不会触发目标程序的快捷键。
#[cfg(windows)]
fn simulate_ctrl_key(key: u16, name: &str) -> Result<(), String> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VK_CONTROL,
    };
    let make = |vk: u16, keyup: bool| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
            ki: KEYBDINPUT {
                wVk: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY(vk),
                wScan: 0,
                dwFlags: if keyup { KEYEVENTF_KEYUP } else { Default::default() },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let seq: [(u16, bool); 4] = [
        (VK_CONTROL.0, false),
        (key, false),
        (key, true),
        (VK_CONTROL.0, true),
    ];
    unsafe {
        for (vk, up) in seq {
            let input = make(vk, up);
            let sent = SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
            if sent != 1 {
                return Err(format!("Ctrl+{name} 发送失败: {sent}"));
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }
    Ok(())
}

const VK_A: u16 = 0x41;
const VK_C: u16 = 0x43;
const VK_V: u16 = 0x56;

/// 全选：抓取与写回输入框内容时都用它
#[cfg(windows)]
pub(crate) fn simulate_ctrl_a() -> Result<(), String> {
    simulate_ctrl_key(VK_A, "A")
}

#[cfg(windows)]
pub(crate) fn simulate_ctrl_c() -> Result<(), String> {
    simulate_ctrl_key(VK_C, "C")
}

/// 粘贴：写回译文时用
#[cfg(windows)]
pub(crate) fn simulate_ctrl_v() -> Result<(), String> {
    simulate_ctrl_key(VK_V, "V")
}

/// 剪贴板法抓取选中文本
fn capture_selection() -> Option<String> {
    // 先记下目标窗口，取词成功后再落库，供「替换原文」回焦
    let target = foreground_hwnd();
    let mut cb = arboard::Clipboard::new().map_err(|e| log_line(&format!("capture: 剪贴板打开失败 {e}"))).ok()?;
    let prev = cb.get_text().ok();
    log_line(&format!(
        "capture: 原剪贴板={}，已清空并发送 Ctrl+C",
        match &prev { Some(t) => t.chars().count().to_string(), None => "空/非文本".into() }
    ));

    cb.clear().ok()?;

    if let Err(e) = simulate_ctrl_c() {
        log_line(&format!("capture: 模拟按键失败 {e}"));
        // 尽力恢复剪贴板后退出
        if let Some(p) = &prev {
            let _ = cb.set_text(p.clone());
        }
        return None;
    }
    std::thread::sleep(Duration::from_millis(350));

    let mut now = cb.get_text().ok();
    if now.as_deref().is_none() {
        // 目标应用可能响应慢，再等一轮重试读取
        std::thread::sleep(Duration::from_millis(250));
        now = cb.get_text().ok();
    }
    log_line(&format!(
        "capture: Ctrl+C 后剪贴板={}",
        match &now { Some(t) => format!("{} 字符", t.chars().count()), None => "空/非文本".into() }
    ));

    // 恢复用户剪贴板（仅文本；图片内容暂不恢复）
    if let Some(p) = &prev {
        let _ = cb.set_text(p.clone());
    }

    match now {
        Some(t)
            if !t.trim().is_empty() && prev.as_deref() != Some(t.as_str()) =>
        {
            LAST_TARGET.store(target, Ordering::SeqCst);
            Some(t)
        }
        _ => None,
    }
}

/// 确保弹窗存在并定位到光标附近（越界时往屏幕内收）
pub(crate) fn ensure_popup_at_cursor(app: &AppHandle) -> Result<(), String> {
    let cursor = app.cursor_position().map_err(|e| e.to_string())?;
    let (mut px, mut py) = (cursor.x + 14.0, cursor.y + 14.0);

    // 用主显示器做越界回退（多显示器精确贴合为后续增强）
    if let Ok(Some(m)) = app.primary_monitor() {
        let sf = m.scale_factor();
        let size = m.size();
        let logical_w = size.width as f64 / sf;
        let logical_h = size.height as f64 / sf;
        let (lx, ly) = (px / sf, py / sf);
        if lx + POPUP_W > logical_w {
            px = (logical_w - POPUP_W - 16.0) * sf;
        }
        if ly + POPUP_H > logical_h {
            py = (logical_h - POPUP_H - 16.0) * sf;
        }
    }
    if px < 0.0 {
        px = 8.0;
    }
    if py < 0.0 {
        py = 8.0;
    }

    // 转逻辑坐标（窗口定位用逻辑像素）
    let sf = app
        .primary_monitor()
        .ok()
        .flatten()
        .map(|m| m.scale_factor())
        .unwrap_or(1.0);
    let (lx, ly) = (px / sf, py / sf);

    match app.get_webview_window("popup") {
        Some(win) => {
            let _ = win.set_position(tauri::LogicalPosition::new(lx, ly));
            let _ = win.show();
            let _ = win.set_focus();
            Ok(())
        }
        None => WebviewWindowBuilder::new(app, "popup", WebviewUrl::App("popup.html".into()))
            .title("随译 · 划词翻译")
            .inner_size(POPUP_W, POPUP_H)
            .position(lx, ly)
            .min_inner_size(360.0, 400.0)
            .decorations(false)
            .transparent(true)
            .shadow(true)
            .resizable(true)
            .effects(tauri::utils::config::WindowEffectsConfig {
                effects: vec![tauri::utils::WindowEffect::Acrylic],
                state: None,
                radius: Some(14.0),
                color: None,
                interactive: false,
            })
            .build()
            .map(|_| ())
            .map_err(|e| format!("创建弹窗失败: {e}")),
    }
}
