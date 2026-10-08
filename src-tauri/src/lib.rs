pub mod anki;
pub mod commands;
pub mod config;
pub mod history;
pub mod hotkeys;
pub mod keyring;
pub mod plugin;
pub mod plugin_js;
pub mod screenshot;
pub mod selection;
pub mod speech;
pub mod translator;
pub mod wordbook;
pub mod writeback;

use tauri::utils::config::WindowEffectsConfig;
use tauri::utils::WindowEffect;
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_global_shortcut::ShortcutState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 回调里不再比对写死的组合，而是查「当前生效」那张表，
    // 这样用户改过键之后事件还能落到正确的动作上。
    let hotkey = tauri_plugin_global_shortcut::Builder::new()
        .with_handler(move |app, shortcut, event| {
            if event.state != ShortcutState::Pressed {
                return;
            }
            let action = app
                .state::<hotkeys::HotkeyState>()
                .action_of(shortcut);
            match action.as_deref() {
                Some("selection") => selection::trigger_selection_translate(app.clone()),
                Some("screenshot") => screenshot::trigger_screenshot(app.clone()),
                Some("input") => writeback::trigger_input_translate(app.clone()),
                _ => {}
            }
        });

    let builder = tauri::Builder::default()
        // 单实例守护必须是第一个注册的插件：重复启动时聚焦已有窗口
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(win) = app.get_webview_window("main") {
                let _ = win.show();
                let _ = win.set_focus();
            }
        }))
        .plugin(tauri_plugin_opener::init())
        // 只装 handler，不在插件里注册热键：插件的 with_shortcuts 是「全成或全不成」，
        // 一个冲突会让另外两个一起失效。注册放到 setup 里逐个做，结果存进 HotkeyState。
        .plugin(hotkey.build())
        .manage(hotkeys::HotkeyState::default())
        .manage(std::sync::Mutex::<Option<screenshot::ShotSession>>::new(None))
        .manage(screenshot::OcrState::default());

    builder
        .setup(move |app| {
            let handle = app.handle().clone();

            // 预创建划词弹窗（隐藏）：首次 Alt+D 免去 webview 冷启动，秒出
            WebviewWindowBuilder::new(&handle, "popup", WebviewUrl::App("popup.html".into()))
                .title("随译 · 划词翻译")
                .inner_size(430.0, 540.0)
                .min_inner_size(360.0, 400.0)
                .decorations(false)
                .transparent(true)
                .shadow(true)
                .resizable(true)
                // 系统级磨砂：CSS 的 backdrop-filter 只作用于页面内部，
                // 弹窗背后的桌面要靠 DWM 的 Acrylic 才会被模糊。
                // 系统不支持时静默退回半透明，卡片本身不透明度在 0.84，仍可读。
                .effects(WindowEffectsConfig {
                    effects: vec![WindowEffect::Acrylic],
                    state: None,
                    radius: Some(14.0),
                    color: None,
                    interactive: false,
                })
                .visible(false)
                .build()?;

            // 逐个注册热键：失败的只影响它自己，界面会显示「已被其他程序占用」
            let config = commands::config_dir(&handle)
                .and_then(|dir| config::load_services(&dir))
                .map(|f| f.hotkeys)
                .unwrap_or_default();
            let pairs = hotkeys::effective(&config);
            let status = hotkeys::apply(&handle, &pairs);
            handle.state::<hotkeys::HotkeyState>().store(status);

            // 旧实例退出需要时间，先失败的在后台补注册
            #[cfg(desktop)]
            {
                let handle = handle.clone();
                let pairs = pairs.clone();
                std::thread::spawn(move || {
                    hotkeys::retry_until_ready(
                        &handle,
                        &pairs,
                        6,
                        std::time::Duration::from_millis(2500),
                    );
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
            commands::read_clipboard_text,
            commands::get_settings,
            commands::save_settings,
            commands::hotkey_status,
            commands::retry_hotkeys,
            commands::set_hotkey,
            commands::reset_hotkeys,
            commands::translate_text,
            commands::list_history,
            commands::delete_history,
            commands::clear_history,
            commands::list_plugins,
            commands::set_plugin_enabled,
            commands::create_sample_plugin,
            commands::plugins_dir_path,
            commands::run_action_plugin,
            commands::test_connection,
            commands::save_text_file,
            anki::anki_status,
            wordbook::wordbook_list,
            wordbook::wordbook_add,
            wordbook::wordbook_sync,
            wordbook::wordbook_remove,
            writeback::replace_selection,
            speech::speak_text,
            speech::speech_state,
            speech::pause_speaking,
            speech::resume_speaking,
            speech::stop_speaking,
            speech::list_speech_voices,
            screenshot::get_screenshot,
            screenshot::finish_region,
            screenshot::cancel_screenshot,
            screenshot::start_screenshot,
            screenshot::ocr_last,
            screenshot::ocr_close,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
