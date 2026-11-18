use crate::providers::{all_providers, running_process_names, Game};
use serde::{Deserialize, Serialize};

// Threading: Tauri runs a plain `fn` command inline on the thread that
// handles the IPC message, so a provider that waits on a registry scan, a
// PowerShell spawn or a 4 s network timeout froze the window. Every command
// that does real work below is `async` and hands its body to
// `spawn_blocking` (Tauri's own runtime pool), so the webview thread never
// waits on provider I/O and the per-launcher `provider_data` calls the
// frontend fires together really do run concurrently.

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
    pub can_launch: bool,
    /// False when the launcher has no honest update trigger (Epic): the
    /// frontend should not offer an "Update" action for it.
    pub can_update: bool,
    /// Set when the launcher is installed but reading its game list failed
    /// (corrupt/locked config, ...). `games` is then empty and the frontend
    /// should show "check failed" with this text instead of "no games".
    /// `None` on success, including a successful empty list.
    pub error: Option<String>,
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
        DeleteObject, GetDC, GetDIBits, GetObjectW, ReleaseDC, BITMAP, BITMAPINFO, BITMAPINFOHEADER,
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

    // SHGetFileInfoW wants COM initialised on the calling thread (Microsoft's
    // docs for it say so). These calls now run on pool threads, not the main
    // UI thread that Tauri/WebView2 already set up as an STA, so init here
    // and pair it with CoUninitialize when the guard drops.
    let _com = ComGuard::init();

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

/// RAII COM initialisation for the current thread (apartment-threaded, the
/// model the shell icon APIs expect). Only uninitialises if this call
/// actually added a reference (`S_OK`/`S_FALSE`); an already-initialised
/// thread in a different mode (`RPC_E_CHANGED_MODE`) is left alone.
#[cfg(windows)]
struct ComGuard(bool);

#[cfg(windows)]
impl ComGuard {
    fn init() -> Self {
        use windows_sys::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE};
        // SAFETY: plain FFI call with a null reserved pointer, as documented.
        let hr = unsafe { CoInitializeEx(std::ptr::null(), (COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) as u32) };
        ComGuard(hr >= 0)
    }
}

#[cfg(windows)]
impl Drop for ComGuard {
    fn drop(&mut self) {
        if self.0 {
            // SAFETY: balances the successful CoInitializeEx in `init`.
            unsafe { windows_sys::Win32::System::Com::CoUninitialize() };
        }
    }
}

#[tauri::command]
pub async fn launcher_icon(launcher: String) -> Option<String> {
    tauri::async_runtime::spawn_blocking(move || {
        let provider = all_providers().into_iter().find(|p| p.id() == launcher)?;
        let path = provider.icon_source()?;
        extract_icon_data_uri(&path)
    })
    .await
    .unwrap_or(None)
}

#[tauri::command]
pub async fn game_icon(launcher: String, game_id: String) -> Option<String> {
    if !is_safe_id(&game_id) {
        return None;
    }
    tauri::async_runtime::spawn_blocking(move || {
        let provider = all_providers().into_iter().find(|p| p.id() == launcher)?;
        let path = provider.game_icon_source(&game_id)?;
        extract_icon_data_uri(&path)
    })
    .await
    .unwrap_or(None)
}

/// Blocking body of `provider_data`, split out so it runs on a pool thread.
fn provider_data_blocking(launcher: &str) -> Option<ProviderResult> {
    let provider = all_providers().into_iter().find(|p| p.id() == launcher)?;
    if !provider.detect() {
        return None;
    }
    let is_running = if let Some(special) = provider.is_running() {
        special
    } else {
        let running = running_process_names();
        provider.process_names().iter().any(|name| running.contains(&name.to_ascii_lowercase()))
    };
    // A failed read is reported, not swallowed: the frontend can tell
    // "installed, check failed" apart from "installed, zero games".
    let (games, error) = match provider.list_games() {
        Ok(games) => (games, None),
        Err(e) => (Vec::new(), Some(e)),
    };

    Some(ProviderResult {
        id: provider.id(),
        name: provider.display_name(),
        games,
        running: is_running,
        can_launch: provider.supports_launch(),
        can_update: provider.supports_update(),
        error,
    })
}

/// Per-launcher detect() + list_games() + running-check, so the frontend
/// can fire one of these per launcher in parallel and render each as it
/// finishes rather than waiting for the slowest provider. Resolves to None
/// for an undetected launcher (nothing installed) so the frontend can drop
/// it; rejects (Err string) only if the worker thread itself fails.
#[tauri::command]
pub async fn provider_data(launcher: String) -> Result<Option<ProviderResult>, String> {
    tauri::async_runtime::spawn_blocking(move || provider_data_blocking(&launcher))
        .await
        .map_err(|e| format!("provider check failed: {e}"))
}

/// Command-layer guard for ids coming from the frontend. Ids are whatever the
/// providers themselves emit — Prism instance folder names ("Modern
/// Industrialization Extra"), Wargaming folder names, registry subkey names —
/// so this is a deny-list of what is dangerous rather than an allow-list of
/// what looks tidy (an earlier allow-list rejected real ids with spaces and
/// the failure was silent in the UI). Rejected: path separators and drive
/// prefixes (`/ \ :`), characters invalid in Windows file names, control
/// characters including NUL, over-long ids, and anything that is only dots
/// and spaces (Windows strips trailing dots/spaces, so `".. "` and `"..."`
/// can resolve like `".."`). Empty stays allowed: it means "the launcher
/// itself" (the Open button).
///
/// Ids that end up inside a protocol URI (steam://, goggalaxy://,
/// com.epicgames.launcher://) are additionally held to the strict
/// `providers::uri_safe_id` shape by those providers, so loosening this
/// layer does not loosen what reaches a URI.
fn is_safe_id(id: &str) -> bool {
    if id.is_empty() {
        return true;
    }
    if id.len() > 256 {
        return false;
    }
    if id.trim_matches(|c| c == '.' || c == ' ').is_empty() {
        return false;
    }
    !id.chars().any(|c| c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
}

#[tauri::command]
pub async fn trigger_update(launcher: String, game_id: String) -> Result<(), String> {
    if !is_safe_id(&game_id) {
        return Err(format!("invalid game id: {game_id:?}"));
    }
    tauri::async_runtime::spawn_blocking(move || {
        let provider = all_providers()
            .into_iter()
            .find(|p| p.id() == launcher)
            .ok_or_else(|| format!("unknown launcher: {launcher}"))?;
        provider.trigger_update(&game_id)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn launch_game(launcher: String, game_id: String) -> Result<(), String> {
    if !is_safe_id(&game_id) {
        return Err(format!("invalid game id: {game_id:?}"));
    }
    tauri::async_runtime::spawn_blocking(move || {
        let provider = all_providers()
            .into_iter()
            .find(|p| p.id() == launcher)
            .ok_or_else(|| format!("unknown launcher: {launcher}"))?;
        provider.launch(&game_id)
    })
    .await
    .map_err(|e| e.to_string())?
}

// ---------------------------------------------------------------- free space
/// One pending game as the frontend knows it. `bytes` is the launcher-reported
/// download size (`update_bytes_total`), `None` when it has none.
#[derive(Deserialize)]
pub struct PendingItem {
    pub launcher: String,
    pub game_id: String,
    pub bytes: Option<u64>,
}

/// Pending downloads that land on one drive.
#[derive(Serialize, Debug, PartialEq, Eq)]
pub struct DriveSpace {
    /// `"D:"`.
    pub drive: String,
    /// Sum of the known download sizes for games on this drive.
    pub required_bytes: u64,
    /// Games on this drive whose size is unknown (not in `required_bytes`).
    pub unknown_size: usize,
    pub games: usize,
    /// Free bytes available to the current user; `None` if the OS call failed.
    pub free_bytes: Option<u64>,
}

#[derive(Serialize)]
pub struct SpaceReport {
    pub drives: Vec<DriveSpace>,
    /// Games whose download drive is not known (non-Steam launchers, UNC
    /// libraries, game not found). They are not part of any drive total.
    pub unplaced: usize,
}

/// `"X:/Steam"` / `"d:\\SteamLibrary"` -> `"X:"` / `"D:"`. UNC and relative
/// paths -> None.
fn drive_of(path: &str) -> Option<String> {
    let mut chars = path.chars();
    let letter = chars.next().filter(|c| c.is_ascii_alphabetic())?;
    (chars.next() == Some(':')).then(|| format!("{}:", letter.to_ascii_uppercase()))
}

/// Groups `(drive, bytes)` pairs by drive, sorted by drive letter. Pairs with
/// no drive count toward the returned `unplaced`. Free space is filled in
/// separately so this stays pure.
fn group_by_drive(items: &[(Option<String>, Option<u64>)]) -> (Vec<DriveSpace>, usize) {
    let mut drives: Vec<DriveSpace> = Vec::new();
    let mut unplaced = 0;
    for (drive, bytes) in items {
        let Some(drive) = drive else {
            unplaced += 1;
            continue;
        };
        let at = match drives.iter().position(|d| &d.drive == drive) {
            Some(i) => i,
            None => {
                drives.push(DriveSpace { drive: drive.clone(), required_bytes: 0, unknown_size: 0, games: 0, free_bytes: None });
                drives.len() - 1
            }
        };
        let d = &mut drives[at];
        d.games += 1;
        match bytes {
            Some(b) => d.required_bytes = d.required_bytes.saturating_add(*b),
            None => d.unknown_size += 1,
        }
    }
    drives.sort_by(|a, b| a.drive.cmp(&b.drive));
    (drives, unplaced)
}

/// `GetDiskFreeSpaceExW` (lpFreeBytesAvailableToCaller) for `"D:"`.
/// https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-getdiskfreespaceexw
#[cfg(windows)]
fn free_bytes_on(drive: &str) -> Option<u64> {
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let root: Vec<u16> = format!("{drive}\\").encode_utf16().chain(std::iter::once(0)).collect();
    let mut free: u64 = 0;
    // SAFETY: `root` is NUL-terminated and outlives the call; the two unused
    // out-pointers are documented as optional (null).
    let ok = unsafe { GetDiskFreeSpaceExW(root.as_ptr(), &mut free, std::ptr::null_mut(), std::ptr::null_mut()) };
    (ok != 0).then_some(free)
}

#[cfg(not(windows))]
fn free_bytes_on(_drive: &str) -> Option<u64> {
    None
}

/// For the Update-all confirmation: where will the pending downloads land and
/// how much room is there. Only launchers that report an install root
/// (Steam) are placed on a drive; everything else is `unplaced`.
#[tauri::command]
pub async fn update_space(items: Vec<PendingItem>) -> Result<SpaceReport, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let providers = all_providers();
        let placed: Vec<(Option<String>, Option<u64>)> = items
            .iter()
            .map(|item| {
                let drive = is_safe_id(&item.game_id)
                    .then(|| providers.iter().find(|p| p.id() == item.launcher))
                    .flatten()
                    .and_then(|p| p.install_root(&item.game_id))
                    .and_then(|root| drive_of(&root.to_string_lossy()));
                (drive, item.bytes)
            })
            .collect();
        let (mut drives, unplaced) = group_by_drive(&placed);
        for d in &mut drives {
            d.free_bytes = free_bytes_on(&d.drive);
        }
        SpaceReport { drives, unplaced }
    })
    .await
    .map_err(|e| format!("free-space check failed: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drive_of_takes_the_letter_from_steam_style_paths() {
        assert_eq!(drive_of("X:/Steam"), Some("X:".to_string()));
        assert_eq!(drive_of("d:\\SteamLibrary"), Some("D:".to_string()));
        assert_eq!(drive_of("C:"), Some("C:".to_string()));
    }

    #[test]
    fn drive_of_rejects_unc_relative_and_empty() {
        assert_eq!(drive_of("\\\\server\\share"), None);
        assert_eq!(drive_of("Steam/lib"), None);
        assert_eq!(drive_of("1:/x"), None);
        assert_eq!(drive_of(""), None);
    }

    #[test]
    fn grouping_sums_known_sizes_counts_unknown_and_unplaced() {
        let d = |s: &str| Some(s.to_string());
        let items = [(d("X:"), Some(100)), (d("D:"), Some(5)), (d("X:"), None), (d("X:"), Some(50)), (None, Some(7)), (None, None)];
        let (drives, unplaced) = group_by_drive(&items);
        assert_eq!(unplaced, 2);
        assert_eq!(
            drives,
            vec![
                DriveSpace { drive: "D:".into(), required_bytes: 5, unknown_size: 0, games: 1, free_bytes: None },
                DriveSpace { drive: "X:".into(), required_bytes: 150, unknown_size: 1, games: 3, free_bytes: None },
            ]
        );
    }

    #[test]
    fn grouping_nothing_is_empty() {
        let (drives, unplaced) = group_by_drive(&[]);
        assert!(drives.is_empty());
        assert_eq!(unplaced, 0);
    }

    #[test]
    fn grouping_does_not_overflow_on_huge_sizes() {
        let d = Some("X:".to_string());
        let (drives, _) = group_by_drive(&[(d.clone(), Some(u64::MAX)), (d, Some(10))]);
        assert_eq!(drives[0].required_bytes, u64::MAX);
    }

    #[cfg(windows)]
    #[test]
    fn free_bytes_on_the_system_drive_is_readable() {
        let sys = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".to_string());
        assert!(free_bytes_on(&sys).is_some_and(|b| b > 0));
    }

    #[test]
    fn empty_string_is_safe() {
        assert!(is_safe_id(""));
    }

    #[test]
    fn alphanumerics_and_allowed_punctuation_are_safe() {
        assert!(is_safe_id("12345"));
        assert!(is_safe_id("steam-76561198000000000"));
        assert!(is_safe_id("some_id.v2"));
    }

    #[test]
    fn exact_dot_and_dotdot_are_rejected() {
        assert!(!is_safe_id("."));
        assert!(!is_safe_id(".."));
    }

    #[test]
    fn dot_and_dotdot_embedded_in_a_longer_id_are_still_safe() {
        // Only ids made purely of dots/spaces are path-traversal special —
        // a longer id with real characters is not.
        assert!(is_safe_id("..foo"));
        assert!(is_safe_id("a.b..c"));
    }

    #[test]
    fn ids_every_provider_really_emits_are_accepted() {
        // Prism: instance folder name, verified on the reference machine.
        assert!(is_safe_id("Modern Industrialization Extra"));
        assert!(is_safe_id("1.21.11"));
        // Prism synthetic app row, Wargaming folder-name fallback, Ubisoft/EA/GOG/Epic/Steam shapes.
        assert!(is_safe_id("__self__"));
        assert!(is_safe_id("World of Tanks EU"));
        assert!(is_safe_id("steam-730"));
        assert!(is_safe_id("1207658762"));
        assert!(is_safe_id("0449f415f5404df093b8d67c31940024"));
        assert!(is_safe_id("FabPlugin_5.8"));
        assert!(is_safe_id("{3A2F1C4E-0000-0000-0000-000000000000}"));
        assert!(is_safe_id("wow_classic"));
        assert!(is_safe_id("league_of_legends.na1"));
        // Non-ASCII folder names are legitimate on Windows.
        assert!(is_safe_id("Mundo Cúbico"));
    }

    #[test]
    fn traversal_separators_and_drive_prefixes_are_rejected() {
        for id in [
            ".", "..", "...", " . ", ".. ", "../x", "..\\x", "a/b", "a\\b", "/abs", "\\\\server\\share", "C:", "C:\\x", "C:x",
            "a:b", "a\0b", "\0", "a\nb", "a\tb",
        ] {
            assert!(!is_safe_id(id), "expected {id:?} to be rejected");
        }
    }

    #[test]
    fn characters_invalid_in_windows_names_are_rejected() {
        for id in ["a*b", "a?b", "a\"b", "a<b", "a>b", "a|b"] {
            assert!(!is_safe_id(id), "expected {id:?} to be rejected");
        }
    }

    #[test]
    fn overlong_ids_are_rejected() {
        assert!(is_safe_id(&"a".repeat(256)));
        assert!(!is_safe_id(&"a".repeat(257)));
    }

    #[cfg(windows)]
    #[test]
    fn icon_extraction_works_off_the_main_thread() {
        // Real check of the async-command change: icon extraction now runs on
        // pool threads, so prove it still yields a real icon from one.
        let system_root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_string());
        let exe = std::path::PathBuf::from(system_root).join("System32\\notepad.exe");
        if !exe.exists() {
            return;
        }
        let uri = std::thread::spawn(move || extract_icon_data_uri(&exe)).join().unwrap();
        assert!(uri.is_some_and(|u| u.starts_with("data:image/bmp;base64,")));
    }
}

