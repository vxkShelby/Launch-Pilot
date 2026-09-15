mod commands;
mod ini;
mod providers;
mod vdf;

// panic = "abort" in the release profile still runs this hook first, it just
// skips unwinding afterward — so a panic still lands one line in the log
// before the process dies, instead of vanishing silently for the user.
fn install_crash_logger() {
    std::panic::set_hook(Box::new(|info| {
        let dir = std::env::var("APPDATA")
            .map(|p| std::path::PathBuf::from(p).join("LaunchPilot"))
            .unwrap_or_else(|_| std::path::PathBuf::from("."));
        let _ = std::fs::create_dir_all(&dir);
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("crash.log")) {
            use std::io::Write;
            let _ = writeln!(f, "[{ts}] {info}");
        }
    }));
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    install_crash_logger();
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .invoke_handler(tauri::generate_handler![
            commands::launcher_ids,
            commands::launcher_icon,
            commands::game_icon,
            commands::provider_data,
            commands::trigger_update,
            commands::launch_game
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
