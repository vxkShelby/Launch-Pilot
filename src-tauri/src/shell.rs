//! Opt-in desktop-shell features: tray icon, close-to-tray, local toast
//! notifications and "start with Windows". Everything here is OFF by default
//! and nothing leaves the machine: notifications are local OS toasts and the
//! settings live in `%APPDATA%\LaunchPilot\settings.json`.
//!
//! Docs used: https://v2.tauri.app/learn/system-tray/ ,
//! https://v2.tauri.app/plugin/notification/ , https://v2.tauri.app/plugin/autostart/

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_autostart::ManagerExt as _;
use tauri_plugin_notification::NotificationExt as _;

const TRAY_ID: &str = "main";
const MAIN_WINDOW: &str = "main";
/// Frontend listens for this and runs its normal refresh.
pub const REFRESH_EVENT: &str = "lp-refresh";

/// Persisted opt-ins. Autostart is deliberately not stored here: the
/// registry Run entry is the source of truth (see `get_settings`).
#[derive(Serialize, Deserialize, Clone, Default, PartialEq, Debug)]
#[serde(default)]
pub struct Settings {
    pub tray: bool,
    pub notify: bool,
}

#[derive(Serialize)]
pub struct SettingsView {
    tray: bool,
    notify: bool,
    autostart: bool,
}

pub struct Shell {
    settings: Mutex<Settings>,
    pending: Mutex<Option<u32>>,
}

impl Shell {
    pub fn new(settings: Settings) -> Self {
        Shell { settings: Mutex::new(settings), pending: Mutex::new(None) }
    }
    fn settings(&self) -> Settings {
        self.settings.lock().map(|s| s.clone()).unwrap_or_default()
    }
}

fn settings_path() -> PathBuf {
    crate::data_dir().join("settings.json")
}

/// A missing or corrupt file means defaults (everything off).
pub fn load() -> Settings {
    std::fs::read_to_string(settings_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save(s: &Settings) -> Result<(), String> {
    let path = settings_path();
    let dir = path.parent().ok_or("no settings directory")?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    // Write-then-rename so a crash mid-write can't leave a truncated file.
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(s).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())
}

pub fn tooltip(pending: Option<u32>) -> String {
    match pending {
        None => "LaunchPilot".into(),
        Some(0) => "LaunchPilot - no updates available".into(),
        Some(1) => "LaunchPilot - 1 update available".into(),
        Some(n) => format!("LaunchPilot - {n} updates available"),
    }
}

/// Toast text for newly found updates: one batched toast, at most 3 names.
pub fn toast_body(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [one] => format!("{one}: update available"),
        many => {
            let shown: Vec<&str> = many.iter().take(3).map(|s| s.as_str()).collect();
            let extra = many.len() - shown.len();
            let tail = if extra > 0 { format!(" +{extra} more") } else { String::new() };
            format!("{} updates available: {}{}", many.len(), shown.join(", "), tail)
        }
    }
}

pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(MAIN_WINDOW) {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

fn toggle_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(MAIN_WINDOW) {
        let visible = w.is_visible().unwrap_or(true) && !w.is_minimized().unwrap_or(false);
        if visible {
            let _ = w.hide();
        } else {
            show_main(app);
        }
    }
}

pub fn tray_exists(app: &AppHandle) -> bool {
    app.tray_by_id(TRAY_ID).is_some()
}

pub fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    if tray_exists(app) {
        return Ok(());
    }
    let open = MenuItem::with_id(app, "open", "Open LaunchPilot", true, None::<&str>)?;
    let refresh = MenuItem::with_id(app, "refresh", "Refresh now", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &refresh, &quit])?;
    let pending = app.state::<Shell>().pending.lock().ok().and_then(|p| *p);
    let mut b = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip(tooltip(pending))
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, ev| match ev.id.as_ref() {
            "open" => show_main(app),
            "refresh" => {
                let _ = app.emit(REFRESH_EVENT, ());
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, ev| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = ev {
                toggle_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        b = b.icon(icon.clone());
    }
    b.build(app)?;
    Ok(())
}

fn remove_tray(app: &AppHandle) {
    let _ = app.remove_tray_by_id(TRAY_ID);
}

/// True when closing the main window should hide it instead of quitting:
/// only with the opt-in on AND a tray icon that actually exists (never hide
/// into a tray that failed to build).
pub fn hide_on_close(app: &AppHandle) -> bool {
    app.state::<Shell>().settings().tray && tray_exists(app)
}

/// Called from `setup`. Returns true if the window should stay hidden
/// (autostart's `--minimized` launch, only when the tray is on and built).
pub fn init(app: &AppHandle) -> bool {
    let settings = load();
    app.manage(Shell::new(settings.clone()));
    if settings.tray {
        if let Err(e) = build_tray(app) {
            crate::log_crash(&format!("tray: {e}"));
        }
    }
    let handle = app.clone();
    // A click on a toast brings the window forward (the plugin reports it
    // while the toast is on screen; not once it has moved to Action Center).
    let _ = app.notification().on_action(move |_| show_main(&handle));
    std::env::args().any(|a| a == "--minimized") && tray_exists(app)
}

/// The autostart crate writes the Run value unquoted (`C:\Users\A B\x.exe
/// --minimized`), which is ambiguous when the path has spaces. Re-write it
/// quoted. Best effort: the value it replaces is still valid for space-free paths.
#[cfg(windows)]
fn quote_run_entry(value_name: &str) {
    use winreg::{enums::*, RegKey};
    let Ok(exe) = std::env::current_exe() else { return };
    if let Ok(run) = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags(r"Software\Microsoft\Windows\CurrentVersion\Run", KEY_SET_VALUE)
    {
        let _ = run.set_value(value_name, &format!("\"{}\" --minimized", exe.display()));
    }
}

fn view(app: &AppHandle) -> SettingsView {
    let s = app.state::<Shell>().settings();
    SettingsView { tray: s.tray, notify: s.notify, autostart: app.autolaunch().is_enabled().unwrap_or(false) }
}

#[tauri::command]
pub fn get_settings(app: AppHandle) -> SettingsView {
    view(&app)
}

#[tauri::command]
pub fn set_setting(app: AppHandle, key: String, value: bool) -> Result<SettingsView, String> {
    match key.as_str() {
        "tray" | "notify" => {
            let shell = app.state::<Shell>();
            let mut next = shell.settings();
            if key == "tray" {
                next.tray = value;
            } else {
                next.notify = value;
            }
            if key == "tray" && value {
                build_tray(&app).map_err(|e| format!("could not create the tray icon: {e}"))?;
            }
            if let Err(e) = save(&next) {
                if key == "tray" && value {
                    remove_tray(&app);
                }
                return Err(e);
            }
            *shell.settings.lock().map_err(|e| e.to_string())? = next;
            if key == "tray" && !value {
                remove_tray(&app);
            }
        }
        "autostart" => {
            let al = app.autolaunch();
            if value { al.enable() } else { al.disable() }.map_err(|e| e.to_string())?;
            #[cfg(windows)]
            if value {
                quote_run_entry(&app.package_info().name);
            }
        }
        _ => return Err(format!("unknown setting: {key}")),
    }
    Ok(view(&app))
}

/// Frontend reports the real count of pending (non-ignored) updates.
#[tauri::command]
pub fn set_pending(app: AppHandle, count: u32) {
    let shell = app.state::<Shell>();
    if let Ok(mut p) = shell.pending.lock() {
        *p = Some(count);
    }
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_tooltip(Some(tooltip(Some(count))));
    }
}

/// One batched local toast for newly found updates. No-op unless the user
/// opted in (checked here, not only in the frontend).
#[tauri::command]
pub fn notify_updates(app: AppHandle, names: Vec<String>) -> Result<(), String> {
    if names.is_empty() || !app.state::<Shell>().settings().notify {
        return Ok(());
    }
    app.notification()
        .builder()
        .title("LaunchPilot")
        .body(toast_body(&names))
        .show()
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tooltip_text() {
        assert_eq!(tooltip(None), "LaunchPilot");
        assert_eq!(tooltip(Some(0)), "LaunchPilot - no updates available");
        assert_eq!(tooltip(Some(1)), "LaunchPilot - 1 update available");
        assert_eq!(tooltip(Some(4)), "LaunchPilot - 4 updates available");
    }

    #[test]
    fn toast_text_is_batched() {
        let n = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(toast_body(&n(&["CS2"])), "CS2: update available");
        assert_eq!(toast_body(&n(&["A", "B"])), "2 updates available: A, B");
        assert_eq!(toast_body(&n(&["A", "B", "C", "D", "E"])), "5 updates available: A, B, C +2 more");
    }

    #[test]
    fn settings_default_off_and_tolerate_garbage() {
        assert_eq!(Settings::default(), Settings { tray: false, notify: false });
        let partial: Settings = serde_json::from_str(r#"{"tray":true}"#).unwrap();
        assert!(partial.tray && !partial.notify);
        assert!(serde_json::from_str::<Settings>("not json").is_err());
    }
}
