//! 光标旁的轻提示窗口。
//!
//! 用在「不能抢焦点、又必须说一句」的地方：目前是首次把窗口收进托盘时的引导
//! （托盘图标在右下角，很多人第一次找不到窗口去哪了）。这里开一个小窗口贴在光标旁，
//! 两秒后自己消失。它透明、置顶、跳过任务栏、**不抢焦点**、鼠标穿透 —— 一律不可交互，
//! 所以不会打断用户正在敲的那个输入框，这正是它和划词弹窗的区别。

use std::sync::Mutex;
use tauri::utils::config::WindowEffectsConfig;
use tauri::utils::WindowEffect;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

const NOTICE_W: f64 = 420.0;
const NOTICE_H: f64 = 66.0;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoticePayload {
    pub text: String,
    /// ok / err / info
    pub kind: String,
}

/// 最近一条提示。轻提示窗口挂载时先取它，事件比页面先到也不会丢内容
#[derive(Default)]
pub struct NoticeState(pub Mutex<Option<NoticePayload>>);

/// 在光标旁显示一条提示。kind: ok | err | info
///
/// 只报信，不往上抛 Result：提示窗口建不出来（极端情况下 DWM 出错）没有补救动作，
/// 记进日志就够了；调用点全是「这条流程的最后一句」，没有地方处理错误。
pub fn show(app: &AppHandle, text: &str, kind: &str) {
    if let Err(e) = show_inner(app, text, kind) {
        crate::selection::log_line(&format!("notice: 显示失败 {e}"));
    }
}

fn show_inner(app: &AppHandle, text: &str, kind: &str) -> Result<(), String> {
    let payload = NoticePayload {
        text: text.to_string(),
        kind: kind.to_string(),
    };
    if let Ok(mut g) = app.state::<NoticeState>().0.lock() {
        *g = Some(payload.clone());
    }

    let cursor = app.cursor_position().map_err(|e| e.to_string())?;
    // 跟随光标所在的那块显示器收边界；多屏下别用主屏边界去 clamp
    let target = app
        .monitor_from_point(cursor.x, cursor.y)
        .ok()
        .flatten()
        .or_else(|| app.primary_monitor().ok().flatten());
    let scale = target.as_ref().map(|m| m.scale_factor()).unwrap_or(1.0);
    let (lx, ly) = {
        // 光标右下角一点，压不到用户正在看的那一行
        let mut lx = cursor.x / scale + 16.0;
        let mut ly = cursor.y / scale + 22.0;
        if let Some(m) = &target {
            let pos = m.position();
            let size = m.size();
            let mx = pos.x as f64 / scale;
            let my = pos.y as f64 / scale;
            let mw = size.width as f64 / scale;
            let mh = size.height as f64 / scale;
            if lx + NOTICE_W > mx + mw {
                lx = mx + mw - NOTICE_W - 12.0;
            }
            if ly + NOTICE_H > my + mh {
                ly = my + mh - NOTICE_H - 12.0;
            }
            lx = lx.max(mx + 8.0);
            ly = ly.max(my + 8.0);
        }
        (lx.max(8.0), ly.max(8.0))
    };

    match app.get_webview_window("notice") {
        Some(win) => {
            let _ = win.set_position(tauri::LogicalPosition::new(lx, ly));
            let _ = win.show();
            let _ = app.emit_to("notice", "notice-show", &payload);
            Ok(())
        }
        None => {
            let win = WebviewWindowBuilder::new(app, "notice", WebviewUrl::App("notice.html".into()))
                .title("随译 · 提示")
                .inner_size(NOTICE_W, NOTICE_H)
                .position(lx, ly)
                .decorations(false)
                .transparent(true)
                .shadow(false)
                .resizable(false)
                .always_on_top(true)
                .skip_taskbar(true)
                .focused(false)
                .visible(false)
                .effects(WindowEffectsConfig {
                    effects: vec![WindowEffect::Acrylic],
                    state: None,
                    radius: Some(10.0),
                    color: None,
                    interactive: false,
                })
                .build()
                .map_err(|e| format!("创建提示窗口失败: {e}"))?;
            // 不抢焦点 + 鼠标穿透：提示只报信，不该改变用户正在输入的那个窗口的焦点，
            // 也不该挡住它下面的点击
            let _ = win.set_focusable(false);
            let _ = win.set_ignore_cursor_events(true);
            let _ = win.show();
            let _ = app.emit_to("notice", "notice-show", &payload);
            Ok(())
        }
    }
}

/// 最近一条提示，供轻提示窗口挂载时兜底读取
#[tauri::command]
pub fn notice_last(state: tauri::State<'_, NoticeState>) -> Option<NoticePayload> {
    state.0.lock().ok().and_then(|g| g.clone())
}

/// 关掉轻提示（前端倒计时结束后自己调，或用户按 Esc）
#[tauri::command]
pub fn notice_hide(app: AppHandle, state: tauri::State<'_, NoticeState>) {
    if let Some(win) = app.get_webview_window("notice") {
        let _ = win.hide();
    }
    if let Ok(mut g) = state.0.lock() {
        *g = None;
    }
}
