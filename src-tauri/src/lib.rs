pub mod commands;
pub mod config;
pub mod keyring;
pub mod selection;
pub mod translator;

// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            // M1：注册全局热键 Alt+D → 划词翻译
            // 注册失败（如另一个实例还占着热键）只告警，不让整个应用崩溃
            #[cfg(desktop)]
            {
                use tauri_plugin_global_shortcut::Builder as GlobalShortcutBuilder;
                match GlobalShortcutBuilder::new().with_shortcuts(["alt+d"]) {
                    Ok(builder) => {
                        if let Err(e) = app.handle().plugin(
                            builder
                                .with_handler(|app, _shortcut, event| {
                                    if event.state
                                        == tauri_plugin_global_shortcut::ShortcutState::Pressed
                                    {
                                        selection::trigger_selection_translate(app.clone());
                                    }
                                })
                                .build(),
                        ) {
                            let msg = format!("全局热键注册失败（可能有另一个随译实例在运行）: {e}");
                            eprintln!("{msg}");
                            selection::log_line(&msg);
                        } else {
                            selection::log_line("hotkey: Alt+D 注册成功");
                        }
                    }
                    Err(e) => {
                        let msg = format!("快捷键解析失败: {e}");
                        eprintln!("{msg}");
                        selection::log_line(&msg);
                    }
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            greet,
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
