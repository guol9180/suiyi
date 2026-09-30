//! M1 划词取词：全局热键触发 → 剪贴板法抓取选中文本 → 光标处弹出翻译窗
//!
//! 策略（与 Pot 相同的兜底方案）：
//! 1. 记录当前剪贴板内容并清空；
//! 2. 模拟 Ctrl+C 让目标应用把选中文本写入剪贴板；
//! 3. 读回剪贴板：非空且与原内容不同 → 视为选中文本；
//! 4. 恢复原剪贴板（不打扰用户）。
//! UIA 直读是后续增强，不阻塞本里程碑。

use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

const POPUP_W: f64 = 430.0;
const POPUP_H: f64 = 540.0;

/// 热键入口：抓取选中文本并弹出翻译窗（重活放独立线程，不阻塞热键处理）
pub fn trigger_selection_translate(app: AppHandle) {
    std::thread::spawn(move || {
        let Some(text) = capture_selection() else { return };
        if ensure_popup_at_cursor(&app).is_err() {
            return;
        }
        // 等弹窗 webview 完成挂载监听
        std::thread::sleep(Duration::from_millis(150));
        let _ = app.emit(
            "popup-set-source",
            serde_json::json!({ "text": text, "autoTranslate": true }),
        );
    });
}

/// 剪贴板法抓取选中文本
fn capture_selection() -> Option<String> {
    let mut cb = arboard::Clipboard::new().ok()?;
    let prev = cb.get_text().ok();

    cb.clear().ok()?;

    let mut enigo = enigo::Enigo::new(&enigo::Settings::default()).ok()?;
    use enigo::{Direction, Key, Keyboard};
    enigo.key(Key::Control, Direction::Press).ok()?;
    enigo.key(Key::Unicode('c'), Direction::Click).ok()?;
    enigo.key(Key::Control, Direction::Release).ok()?;
    std::thread::sleep(Duration::from_millis(240));

    let now = cb.get_text().ok();

    // 恢复用户剪贴板（仅文本；图片内容暂不恢复）
    if let Some(p) = &prev {
        let _ = cb.set_text(p.clone());
    }

    match now {
        Some(t)
            if !t.trim().is_empty() && prev.as_deref() != Some(t.as_str()) =>
        {
            Some(t)
        }
        _ => None,
    }
}

/// 确保弹窗存在并定位到光标附近（越界时往屏幕内收）
fn ensure_popup_at_cursor(app: &AppHandle) -> Result<(), String> {
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
            .build()
            .map(|_| ())
            .map_err(|e| format!("创建弹窗失败: {e}")),
    }
}
