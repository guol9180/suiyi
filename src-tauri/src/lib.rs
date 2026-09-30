pub mod commands;
pub mod config;
pub mod keyring;
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
