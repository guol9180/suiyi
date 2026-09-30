pub mod commands;
pub mod config;
pub mod history;
pub mod keyring;
pub mod screenshot;
pub mod selection;
pub mod speech;
pub mod translator;
pub mod writeback;

use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_global_shortcut::ShortcutState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 热键比对用的目标定义
    let hk_d: tauri_plugin_global_shortcut::Shortcut = "alt+d".parse().expect("解析 alt+d");
    let hk_s: tauri_plugin_global_shortcut::Shortcut = "alt+s".parse().expect("解析 alt+s");
    let hk_t: tauri_plugin_global_shortcut::Shortcut = "alt+t".parse().expect("解析 alt+t");

    let hotkey = tauri_plugin_global_shortcut::Builder::new()
        .with_handler(move |app, shortcut, event| {
            if event.state != ShortcutState::Pressed {
                return;
            }
            if *shortcut == hk_d {
                selection::trigger_selection_translate(app.clone());
            } else if *shortcut == hk_s {
                screenshot::trigger_screenshot(app.clone());
            } else if *shortcut == hk_t {
                writeback::trigger_input_translate(app.clone());
            }
        })
        .with_shortcuts(["alt+d", "alt+s", "alt+t"]);

    let mut builder = tauri::Builder::default()
        // 单实例守护必须是第一个注册的插件：重复启动时聚焦已有窗口
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(win) = app.get_webview_window("main") {
                let _ = win.show();
                let _ = win.set_focus();
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .manage(std::sync::Mutex::<Option<screenshot::ShotSession>>::new(None));

    // 同步注册热键；失败（如旧实例尚未释放）时后台补注册
    match hotkey {
        Ok(p) => builder = builder.plugin(p.build()),
        Err(e) => eprintln!("全局热键初始化失败: {e}"),
    }

    builder
        .setup(move |app| {
            let handle = app.handle().clone();
            let hk_d = hk_d.clone();
            let hk_s = hk_s.clone();
            let hk_t = hk_t.clone();

            // 预创建划词弹窗（隐藏）：首次 Alt+D 免去 webview 冷启动，秒出
            WebviewWindowBuilder::new(&handle, "popup", WebviewUrl::App("popup.html".into()))
                .title("随译 · 划词翻译")
                .inner_size(430.0, 540.0)
                .min_inner_size(360.0, 400.0)
                .decorations(false)
                .transparent(true)
                .shadow(true)
                .resizable(true)
                .visible(false)
                .build()?;

            // 注册失败的兜底重试（旧实例退出需要时间）
            #[cfg(desktop)]
            {
                let handle = handle.clone();
                std::thread::spawn(move || {
                    use tauri_plugin_global_shortcut::GlobalShortcutExt;
                    for (name, hk) in [
                        ("alt+d", hk_d.clone()),
                        ("alt+s", hk_s.clone()),
                        ("alt+t", hk_t.clone()),
                    ] {
                        let already = handle.global_shortcut().is_registered(hk.clone());
                        if already {
                            continue;
                        }
                        let mut ok = false;
                        for attempt in 1..=6u32 {
                            match handle.global_shortcut().register(hk.clone()) {
                                Ok(_) => {
                                    ok = true;
                                    selection::log_line(&format!(
                                        "hotkey: {name} 补注册成功（第 {attempt} 次尝试）"
                                    ));
                                    break;
                                }
                                Err(e) => {
                                    selection::log_line(&format!(
                                        "hotkey: {name} 第 {attempt} 次补注册失败: {e}"
                                    ));
                                    std::thread::sleep(std::time::Duration::from_millis(2500));
                                }
                            }
                        }
                        let _ = ok;
                    }
                });
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_services,
            commands::save_service,
            commands::delete_service,
            commands::reorder_services,
            commands::set_api_key,
            commands::get_api_key,
            commands::delete_api_key,
            commands::get_settings,
            commands::save_settings,
            commands::translate_text,
            commands::list_history,
            commands::delete_history,
            commands::clear_history,
            commands::test_connection,
            writeback::replace_selection,
            speech::speak_text,
            speech::stop_speaking,
            screenshot::get_screenshot,
            screenshot::finish_region,
            screenshot::cancel_screenshot,
            screenshot::start_screenshot,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
