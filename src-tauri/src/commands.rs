use crate::providers::{all_providers, Game};
use serde::Serialize;

/// Real, single Windows process snapshot via the built-in `tasklist` tool
/// (no extra dependency, works on any Windows install) — one call covers
/// every provider instead of spawning a process per launcher.
///
/// Tried and reverted: gating this on "owns a visible top-level window"
/// (via EnumWindows/IsWindowVisible) to fix a real false positive — Ubisoft
/// Connect's `upc.exe` stays alive as a backgrounded/tray process after the
/// user closes the client, so process-presence alone reports it as running
/// when it isn't. But verified live with a raw EnumWindows dump that this
/// doesn't actually distinguish the cases: Steam, while genuinely open but
/// minimized to tray, owns only the exact same shape of window — hidden
/// (IsWindowVisible=false), with a real title ("Steam" / "Ubisoft
/// Connect"). Windows doesn't expose a reliable "is this a real closed
/// client vs. minimized-to-tray" signal short of parsing the notification
/// area itself, which is out of scope. Reverted to plain process presence:
/// correct for every launcher tested except Ubisoft Connect's tray-resident
/// helper, a disclosed known limitation rather than a fragile heuristic
/// that broke Steam to partially fix Ubisoft.
///
/// Bug fix, verified live: `provider_data` is called once per launcher (10
/// parallel invokes per dashboard load/refresh), and each one used to spawn
/// its own `tasklist` process — 10 child console processes at once on every
/// startup. Two real problems from that, both fixed here: (1) `tasklist`
/// inherits a visible console window unless explicitly told not to (Rust's
/// std::process::Command doesn't set that by default), so the user saw cmd
/// windows flash on launch; (2) spawning + waiting on 10 of them at once is
/// real OS overhead that was visible as LaunchPilot going "Not Responding"
/// momentarily. Fixed by adding the CREATE_NO_WINDOW flag (suppresses the
/// console) and caching the one real snapshot for a couple of seconds so
/// concurrent provider_data calls from the same refresh share it instead of
/// each spawning their own.
fn running_process_names() -> std::collections::HashSet<String> {
    use std::sync::{Mutex, OnceLock};
    use std::time::{Duration, Instant};

    static CACHE: OnceLock<Mutex<Option<(Instant, std::collections::HashSet<String>)>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(None));
    let mut guard = cache.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((fetched_at, names)) = guard.as_ref() {
        if fetched_at.elapsed() < Duration::from_secs(2) {
            return names.clone();
        }
    }

    let mut command = std::process::Command::new("tasklist");
    command.arg("/FO").arg("CSV").arg("/NH");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    let names: std::collections::HashSet<String> = match command.output() {
        Ok(output) => String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| line.split(',').next())
            .map(|name| name.trim_matches('"').to_ascii_lowercase())
            .collect(),
        Err(_) => std::collections::HashSet::new(),
    };
    *guard = Some((Instant::now(), names.clone()));
    names
}

/// Static id list — fast, no registry/network/process work — so the
/// frontend can draw a section per launcher immediately and fill each one
/// in as its own `provider_data` call resolves, instead of one big call
/// blocking the whole dashboard behind whichever provider is slowest
/// (GOG's network round-trip, a big Steam library, etc.).
#[tauri::command]
pub fn launcher_ids() -> Vec<&'static str> {
    all_providers().iter().map(|p| p.id()).collect()
}

#[derive(Serialize)]
pub struct ProviderResult {
    pub id: &'static str,
    pub name: &'static str,
    pub games: Vec<Game>,
    pub running: bool,
}

/// Reads the real icon Windows itself associates with a local exe/file
/// (the exact icon shown in Explorer/the taskbar for that file — via the
/// standard shell + GDI APIs), and returns it as a `data:image/bmp;base64`
/// string the frontend can drop straight into an `<img src>`. No icon is
/// fabricated or guessed: this only ever runs against a path a provider
/// already resolved as its own real, verified client exe. Returns None on
/// any failure (missing file, API error) — the frontend just shows no icon.
#[cfg(windows)]
fn extract_icon_data_uri(path: &std::path::Path) -> Option<String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Graphics::Gdi::{
        DeleteDC, DeleteObject, GetDC, GetDIBits, GetObjectW, ReleaseDC, BITMAP, BITMAPINFO, BITMAPINFOHEADER,
        BI_RGB, DIB_RGB_COLORS,
    };
    use windows_sys::Win32::UI::Shell::{SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_SMALLICON};
    use windows_sys::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, ICONINFO};

    if !path.exists() {
        return None;
    }
    // Some providers build paths by joining a forward-slash-normalized
    // library root (steam.rs's library_paths) with backslash-separated
    // components — valid for std::fs, which accepts either separator, but
    // verified live that SHGetFileInfoW silently fails to resolve an icon
    // for a mixed-separator path ("X:/Steam\steamapps\..."). Normalizing
    // to backslashes only for this Win32 call fixed every Steam/Steam-
    // sourced-Ubisoft game that was failing.
    let normalized = path.to_string_lossy().replace('/', "\\");
    let wide: Vec<u16> = std::ffi::OsStr::new(&normalized).encode_wide().chain(std::iter::once(0)).collect();

    unsafe {
        let mut info: SHFILEINFOW = std::mem::zeroed();
        let ok = SHGetFileInfoW(
            wide.as_ptr(),
            0,
            &mut info,
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_SMALLICON,
        );
        if ok == 0 || info.hIcon.is_null() {
            return None;
        }

        let mut icon_info: ICONINFO = std::mem::zeroed();
        if GetIconInfo(info.hIcon, &mut icon_info) == 0 {
            DestroyIcon(info.hIcon);
            return None;
        }

        let mut bmp: BITMAP = std::mem::zeroed();
        GetObjectW(icon_info.hbmColor as _, std::mem::size_of::<BITMAP>() as i32, &mut bmp as *mut _ as *mut _);
        let width = bmp.bmWidth;
        let height = bmp.bmHeight;
        if width <= 0 || height <= 0 {
            DeleteObject(icon_info.hbmColor as _);
            DeleteObject(icon_info.hbmMask as _);
            DestroyIcon(info.hIcon);
            return None;
        }

        let mut bmi: BITMAPINFO = std::mem::zeroed();
        bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        bmi.bmiHeader.biWidth = width;
        bmi.bmiHeader.biHeight = -height; // request top-down rows
        bmi.bmiHeader.biPlanes = 1;
        bmi.bmiHeader.biBitCount = 32;
        bmi.bmiHeader.biCompression = BI_RGB as u32;

        let mut pixels = vec![0u8; (width as usize) * (height as usize) * 4];
        let hdc = GetDC(std::ptr::null_mut());
        let scanlines = GetDIBits(hdc, icon_info.hbmColor as _, 0, height as u32, pixels.as_mut_ptr() as *mut _, &mut bmi, DIB_RGB_COLORS);
        ReleaseDC(std::ptr::null_mut(), hdc);
        DeleteObject(icon_info.hbmColor as _);
        DeleteObject(icon_info.hbmMask as _);
        DestroyIcon(info.hIcon);
        let _ = DeleteDC; // Gdi import kept for symmetry with other DC calls

        if scanlines == 0 {
            return None;
        }

        // Build a minimal, valid 32bpp BMP file around the pixel data
        // GetDIBits already gave us in Windows' native BGRA order.
        let pixel_data_size = pixels.len() as u32;
        let file_header_size: u32 = 14;
        let info_header_size: u32 = 40;
        let file_size = file_header_size + info_header_size + pixel_data_size;

        let mut bmp_bytes = Vec::with_capacity(file_size as usize);
        bmp_bytes.extend_from_slice(b"BM");
        bmp_bytes.extend_from_slice(&file_size.to_le_bytes());
        bmp_bytes.extend_from_slice(&0u16.to_le_bytes());
        bmp_bytes.extend_from_slice(&0u16.to_le_bytes());
        bmp_bytes.extend_from_slice(&(file_header_size + info_header_size).to_le_bytes());
        bmp_bytes.extend_from_slice(&info_header_size.to_le_bytes());
        bmp_bytes.extend_from_slice(&width.to_le_bytes());
        bmp_bytes.extend_from_slice(&(-height).to_le_bytes()); // negative = top-down in-file too
        bmp_bytes.extend_from_slice(&1u16.to_le_bytes());
        bmp_bytes.extend_from_slice(&32u16.to_le_bytes());
        bmp_bytes.extend_from_slice(&0u32.to_le_bytes());
        bmp_bytes.extend_from_slice(&pixel_data_size.to_le_bytes());
        bmp_bytes.extend_from_slice(&0i32.to_le_bytes());
        bmp_bytes.extend_from_slice(&0i32.to_le_bytes());
        bmp_bytes.extend_from_slice(&0u32.to_le_bytes());
        bmp_bytes.extend_from_slice(&0u32.to_le_bytes());
        bmp_bytes.extend_from_slice(&pixels);

        use base64::Engine;
        Some(format!("data:image/bmp;base64,{}", base64::engine::general_purpose::STANDARD.encode(&bmp_bytes)))
    }
}

#[cfg(not(windows))]
fn extract_icon_data_uri(_path: &std::path::Path) -> Option<String> {
    None
}

#[tauri::command]
pub fn launcher_icon(launcher: String) -> Option<String> {
    let provider = all_providers().into_iter().find(|p| p.id() == launcher)?;
    let path = provider.icon_source()?;
    extract_icon_data_uri(&path)
}

#[tauri::command]
pub fn game_icon(launcher: String, game_id: String) -> Option<String> {
    if !is_safe_id(&game_id) {
        return None;
    }
    let provider = all_providers().into_iter().find(|p| p.id() == launcher)?;
    let path = provider.game_icon_source(&game_id)?;
    extract_icon_data_uri(&path)
}

/// Per-launcher detect() + list_games() + running-check, so the frontend
/// can fire one of these per launcher in parallel and render each as it
/// finishes rather than waiting for the slowest provider. Returns None for
/// an undetected launcher (nothing installed) so the frontend can drop it.
#[tauri::command]
pub fn provider_data(launcher: String) -> Option<ProviderResult> {
    let provider = all_providers().into_iter().find(|p| p.id() == launcher)?;
    if !provider.detect() {
        return None;
    }
    let running = running_process_names();
    let is_running = provider
        .process_names()
        .iter()
        .any(|name| running.contains(&name.to_ascii_lowercase()));
    let games = provider.list_games().unwrap_or_default();

    Some(ProviderResult {
        id: provider.id(),
        name: provider.display_name(),
        games,
        running: is_running,
    })
}

/// Rejects anything that isn't a plausible id before it reaches a shell-out
/// or URI (steam://, com.epicgames.launcher://, goggalaxy://, RiotClient
/// process args) — defense in depth against a compromised/XSS'd frontend
/// calling trigger_update directly with an attacker-chosen game_id.
fn is_safe_id(id: &str) -> bool {
    id.is_empty() || id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

#[tauri::command]
pub fn trigger_update(launcher: String, game_id: String) -> Result<(), String> {
    if !is_safe_id(&game_id) {
        return Err("invalid game id".to_string());
    }
    let provider = all_providers()
        .into_iter()
        .find(|p| p.id() == launcher)
        .ok_or_else(|| format!("unknown launcher: {launcher}"))?;
    provider.trigger_update(&game_id)
}
