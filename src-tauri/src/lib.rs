mod commands;
mod providers;
mod vdf;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .invoke_handler(tauri::generate_handler![
            commands::list_games,
            commands::list_launchers_without_games,
            commands::trigger_update
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
