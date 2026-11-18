mod commands;
mod ini;
mod providers;
mod shell;
mod steam_common;
mod vdf;

/// `%APPDATA%\LaunchPilot` (crash log, settings.json).
pub(crate) fn data_dir() -> std::path::PathBuf {
    std::env::var("APPDATA")
        .map(|p| std::path::PathBuf::from(p).join("LaunchPilot"))
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
}

/// Appends one line to `%APPDATA%\LaunchPilot\crash.log` (best effort).
fn log_crash(line: &str) {
    let dir = data_dir();
    let _ = std::fs::create_dir_all(&dir);
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("crash.log")) {
        use std::io::Write;
        let _ = writeln!(f, "[{ts}] {line}");
    }
}

// panic = "abort" in the release profile still runs this hook first, it just
// skips unwinding afterward — so a panic still lands one line in the log
// before the process dies, instead of vanishing silently for the user.
fn install_crash_logger() {
    std::panic::set_hook(Box::new(|info| log_crash(&info.to_string())));
}

/// The release build is a GUI-subsystem process (`windows_subsystem =
/// "windows"` in main.rs), so a startup failure such as a missing WebView2
/// runtime would otherwise end the process with no visible sign. Show a
/// native message box with the error and where the log is.
#[cfg(windows)]
fn show_fatal_error(message: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
    let wide = |s: &str| -> Vec<u16> { s.encode_utf16().chain(std::iter::once(0)).collect() };
    let text = wide(&format!(
        "LaunchPilot could not start.\n\n{message}\n\nDetails were written to %APPDATA%\\LaunchPilot\\crash.log"
    ));
    let title = wide("LaunchPilot");
    // SAFETY: both buffers are NUL-terminated UTF-16 and outlive the call; a
    // null owner window is allowed.
    unsafe { MessageBoxW(std::ptr::null_mut(), text.as_ptr(), title.as_ptr(), MB_OK | MB_ICONERROR) };
}

#[cfg(not(windows))]
fn show_fatal_error(_message: &str) {}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    install_crash_logger();
    tauri::Builder::default()
        // Must be registered first. A second launch hands off to this
        // instance (which just brings its window forward) and exits, so two
        // copies never run two refresh timers or two self-updaters.
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            // An autostart launch (--minimized) must not pop the window up.
            if !args.iter().any(|a| a == "--minimized") {
                shell::show_main(app);
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_notification::init())
        // Registers/removes the HKCU Run entry; started with --minimized.
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--minimized"]),
        ))
        .setup(|app| {
            use tauri::Manager;
            let stay_hidden = shell::init(app.handle());
            if !stay_hidden {
                shell::show_main(app.handle());
            } else if let Some(w) = app.get_webview_window("main") {
                let _ = w.hide();
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            use tauri::Manager;
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" && shell::hide_on_close(window.app_handle()) {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::launcher_ids,
            commands::launcher_icon,
            commands::game_icon,
            commands::provider_data,
            commands::trigger_update,
            commands::launch_game,
            commands::update_space,
            shell::get_settings,
            shell::set_setting,
            shell::set_pending,
            shell::notify_updates
        ])
        .run(tauri::generate_context!())
        .unwrap_or_else(|e| {
            let message = format!("error while running tauri application: {e}");
            log_crash(&message);
            show_fatal_error(&message);
            std::process::exit(1);
        });
}
