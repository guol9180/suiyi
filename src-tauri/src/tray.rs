//! 系统托盘图标。
//!
//! 随译是个后台常驻的热键工具：窗口收起来之后进程还在跑，得有个看得见、点得到的
//! 落点，用户才知道它没退出，也才有地方真正退出。用 Tauri 内置的托盘（`tray-icon`
//! feature），不引第三方插件。

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager};

pub const TRAY_ID: &str = "suiyi-tray";

/// 建托盘图标。失败不致命：窗口与热键照常工作，只把原因记进日志。
pub fn init(app: &AppHandle) {
    if let Err(e) = build(app) {
        crate::selection::log_line(&format!("tray: 创建托盘图标失败 {e}"));
    }
}

fn build(app: &AppHandle) -> Result<(), String> {
    let show = MenuItem::with_id(app, "show", "显示主窗口", true, None::<&str>)
        .map_err(|e| e.to_string())?;
    let update = MenuItem::with_id(app, "update", "检查更新", true, None::<&str>)
        .map_err(|e| e.to_string())?;
    let quit = MenuItem::with_id(app, "quit", "退出随译", true, None::<&str>)
        .map_err(|e| e.to_string())?;
    let sep = PredefinedMenuItem::separator(app).map_err(|e| e.to_string())?;
    let menu = Menu::with_items(app, &[&show, &update, &sep, &quit]).map_err(|e| e.to_string())?;

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("随译 SuiYi")
        .menu(&menu)
        // 左键单击直接唤回窗口，不弹菜单：这是最常做的一件事
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main(app),
            "update" => {
                show_main(app);
                // 界面收到就切到设置 → 关于并自己查一次
                let _ = app.emit("check-update-requested", ());
            }
            "quit" => {
                crate::selection::log_line("tray: 菜单里选择退出");
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon().cloned() {
        builder = builder.icon(icon);
    }
    builder.build(app).map_err(|e| e.to_string())?;
    crate::selection::log_line("tray: 托盘图标就绪");
    Ok(())
}

/// 唤回主窗口：收进托盘（隐藏）与最小化两种情况都能叫回来
pub fn show_main(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.show();
        let _ = win.unminimize();
        let _ = win.set_focus();
    }
}
