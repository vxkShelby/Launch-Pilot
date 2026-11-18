pub mod ageofthering;
pub mod battlenet;
pub mod ea;
pub mod epic;
pub mod gog;
pub mod prismlauncher;
pub mod riot;
pub mod steam;
pub mod ubisoft;
pub mod wargaming;

use serde::Serialize;

#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "snake_case")]
pub enum UpdateStatus {
    UpToDate,
    UpdateAvailable,
    /// A download/install is actively in progress right now (observed via
    /// the launcher's own live log/state, not inferred) — distinct from
    /// UpdateAvailable, which means "pending, not yet started."
    Updating,
    #[default]
    Unknown,
}

/// One installed game/instance as sent to the frontend. Every optional field
/// serialises as `null` when the launcher has no real, verified source for it
/// (never a guessed/zero value). Which launcher fills which optional field:
///
/// | field                | populated by                                              |
/// |----------------------|-----------------------------------------------------------|
/// | `installed_build`    | Steam (buildid), Epic, GOG, EA, Battle.net, Wargaming, Prism, AotR |
/// | `size_bytes`         | Steam (SizeOnDisk), Epic (InstallSize), EA (EstimatedSize, approx.) |
/// | `last_updated`       | Steam only (LastUpdated, unix seconds)                    |
/// | `last_played`        | Steam (LastPlayed), PrismLauncher instances (lastLaunchTime) |
/// | `install_date`       | GOG only (INSTALLDATE, local time string)                 |
/// | `update_bytes_total`, `update_bytes_done`, `update_scheduled` | Steam only, and only while `status` is `update_available` / `updating` |
///
/// Deliberately NOT populated (no verified source): Epic last-played;
/// Battle.net / Ubisoft / Wargaming sizes (their Uninstall `EstimatedSize` is
/// empty on the reference machine); GOG size (the registry value there is an
/// installer stub, not the game size).
#[derive(Serialize, Clone, Default)]
pub struct Game {
    pub launcher: &'static str,
    pub id: String,
    pub name: String,
    pub installed_build: Option<String>,
    pub status: UpdateStatus,
    pub size_bytes: Option<u64>,
    /// Unix seconds the launcher last patched this game. Steam only.
    pub last_updated: Option<u64>,
    /// Unix seconds the game was last started (per install). Steam, Prism.
    pub last_played: Option<u64>,
    /// GOG's `INSTALLDATE`, `YYYY-MM-DD HH:MM:SS` in the machine's local time.
    /// The string carries no timezone, so it is not converted to an epoch.
    pub install_date: Option<String>,
    /// Steam pending update: bytes Steam says it must download
    /// (`BytesToDownload`). Only set while the status is pending/updating.
    pub update_bytes_total: Option<u64>,
    /// Steam pending update: bytes already downloaded (`BytesDownloaded`).
    pub update_bytes_done: Option<u64>,
    /// Steam pending update: unix seconds Steam plans to start the update
    /// (`ScheduledAutoUpdate`); `null` when nothing is scheduled (0).
    pub update_scheduled: Option<u64>,
}

pub trait LauncherProvider: Send + Sync {
    fn id(&self) -> &'static str;
    fn display_name(&self) -> &'static str;
    /// Whether this launcher is installed on the machine.
    fn detect(&self) -> bool;
    fn list_games(&self) -> Result<Vec<Game>, String>;
    /// Best-effort update trigger. Empty game_id targets the launcher itself
    /// rather than a specific game, for launchers without per-game deep links.
    fn trigger_update(&self, game_id: &str) -> Result<(), String>;
    /// Process image name(s) (as Windows Task Manager/tasklist would show
    /// them) that mean this launcher's client is currently running. Used
    /// only for a live running/not-running indicator, never for detection —
    /// a launcher can be installed but not running. Empty by default for
    /// providers where no exe name has been verified.
    fn process_names(&self) -> &'static [&'static str] {
        &[]
    }
    /// Overrides the generic process_names-based running check when a
    /// launcher needs more than "is the process alive" to answer honestly
    /// (e.g. Ubisoft Connect's upc.exe stays resident as a tray-only helper
    /// after the UI closes — its own command line real-signals this via a
    /// `-upc_desktop_mode` flag, verified live from this machine's own Task
    /// Manager). None means "no special-case check, use process_names."
    fn is_running(&self) -> Option<bool> {
        None
    }
    /// Real local path to this launcher's own client exe (or a registry
    /// DisplayIcon-style file), used only to pull its actual icon for
    /// display — never guessed/hardcoded to a value not backed by a
    /// registry read or a path this provider already resolves elsewhere.
    /// None means "no verified path available," not "no icon exists."
    fn icon_source(&self) -> Option<std::path::PathBuf> {
        None
    }
    /// Real local exe path to use for one specific game's icon (re-derived
    /// per request rather than cached in `Game`, so a payload with many
    /// games doesn't have to carry a path for every one of them up front).
    /// None means "no verified path for this game," not "no icon exists."
    fn game_icon_source(&self, _game_id: &str) -> Option<std::path::PathBuf> {
        None
    }
    /// Launch one specific game's own exe — spawns the exact same real
    /// local path `game_icon_source` already resolves for that game, never
    /// a separate heuristic. Err(_) by default for providers with no
    /// verified per-game exe path, same honesty rule as `game_icon_source`:
    /// no guessing a launch target that was never confirmed on disk.
    fn launch(&self, _game_id: &str) -> Result<(), String> {
        Err("launch not supported for this launcher".to_string())
    }
    /// Whether this provider overrides `launch()` with a real implementation.
    /// The authoritative answer to "can the frontend show a Launch button for
    /// this launcher" — false by default, true only for the providers that
    /// actually override `launch()` above. Exists so the frontend reads this
    /// off `ProviderResult` instead of hand-maintaining its own copy of which
    /// launchers support launching.
    fn supports_launch(&self) -> bool {
        false
    }
    /// Whether `trigger_update` honestly starts or opens an update flow for
    /// this launcher. False only where no verified update-only mechanism
    /// exists and the one known deep link would do something else (Epic: its
    /// only known per-game URI action starts the game).
    fn supports_update(&self) -> bool {
        true
    }
}

/// Ids spliced into a protocol URI (steam://, goggalaxy://,
/// com.epicgames.launcher://): ASCII alphanumerics, `_`, `-`, `.`, non-empty,
/// and not a bare run of dots. Stricter than the command-layer `is_safe_id`
/// on purpose — these ids are numeric/hex/slug in practice.
pub fn uri_safe_id(id: &str) -> Result<&str, String> {
    let ok = !id.is_empty()
        && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
        && id.chars().any(|c| c != '.');
    if ok {
        Ok(id)
    } else {
        Err("invalid game id".to_string())
    }
}

/// Spawns a game/launcher executable with its own folder (or an explicit
/// `cwd`) as working directory — many games load assets relative to it and
/// fail or exit when started from LaunchPilot's directory. Refuses anything
/// that isn't an `.exe` (EA's `DisplayIcon` can point at an icon file).
pub fn spawn_exe(path: &std::path::Path, cwd: Option<&std::path::Path>) -> Result<(), String> {
    if path.extension().and_then(|e| e.to_str()).map(|e| e.eq_ignore_ascii_case("exe")) != Some(true) {
        return Err(format!("not an executable: {}", path.display()));
    }
    let mut command = std::process::Command::new(path);
    if let Some(dir) = cwd.or_else(|| path.parent()).filter(|d| d.is_dir()) {
        command.current_dir(dir);
    }
    command.spawn().map(|_| ()).map_err(|e| e.to_string())
}

/// Windows PowerShell 5.1 by absolute path (`%SystemRoot%\System32\
/// WindowsPowerShell\v1.0\powershell.exe`) instead of a bare `powershell`,
/// which Windows resolves application-directory-first.
pub fn powershell_exe() -> std::path::PathBuf {
    let system_root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_string());
    std::path::PathBuf::from(system_root).join("System32\\WindowsPowerShell\\v1.0\\powershell.exe")
}

/// Drive letters that are safe to probe with `Path::exists()`. Skips network
/// drives (a mapped SMB share whose server is down blocks `exists()` until the
/// redirector times out), CD/DVD, and letters with no drive.
pub fn scannable_drive_letters() -> Vec<char> {
    ('C'..='Z').filter(|d| drive_is_scannable(*d)).collect()
}

#[cfg(windows)]
fn drive_is_scannable(letter: char) -> bool {
    use windows_sys::Win32::Storage::FileSystem::GetDriveTypeW;
    let root: Vec<u16> = format!("{letter}:\\").encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: `root` is a NUL-terminated UTF-16 string that outlives the call.
    let kind = unsafe { GetDriveTypeW(root.as_ptr()) };
    drive_type_is_scannable(kind)
}

#[cfg(not(windows))]
fn drive_is_scannable(_letter: char) -> bool {
    false
}

/// GetDriveType return codes (winbase.h): 0 UNKNOWN, 1 NO_ROOT_DIR,
/// 2 REMOVABLE, 3 FIXED, 4 REMOTE, 5 CDROM, 6 RAMDISK.
fn drive_type_is_scannable(kind: u32) -> bool {
    matches!(kind, 2 | 3 | 6)
}

/// Lower-cased image names of every running process, from the Win32
/// ToolHelp snapshot API. This replaced spawning `tasklist`, which took
/// 45-160 s per call on the reference machine (measured: `tasklist /FO CSV
/// /NH` 162 s vs. `Get-Process` 0.05 s) and, being shared behind one lock,
/// held every launcher's running-check — and so its whole result — for that
/// long. No child process, no console window.
#[cfg(windows)]
fn snapshot_process_names() -> std::collections::HashSet<String> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
    };

    let mut names = std::collections::HashSet::new();
    // SAFETY: standard ToolHelp enumeration. `entry` is zero-initialised
    // with `dwSize` set as the API requires, and the snapshot handle is
    // closed before returning.
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return names;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut more = Process32FirstW(snapshot, &mut entry);
        while more != 0 {
            let len = entry.szExeFile.iter().position(|c| *c == 0).unwrap_or(entry.szExeFile.len());
            names.insert(String::from_utf16_lossy(&entry.szExeFile[..len]).to_ascii_lowercase());
            more = Process32NextW(snapshot, &mut entry);
        }
        CloseHandle(snapshot);
    }
    names
}

#[cfg(not(windows))]
fn snapshot_process_names() -> std::collections::HashSet<String> {
    std::collections::HashSet::new()
}

/// Running process names for the per-launcher running indicator, cached for
/// two seconds so the concurrent `provider_data` calls of one refresh share
/// one snapshot. Plain process presence, not a window check: Ubisoft
/// Connect's tray-resident helper is handled separately (ubisoft.rs
/// `is_running`), and a window-visibility check was tried and reverted
/// because Steam minimized to tray looks identical to a closed client.
pub fn running_process_names() -> std::collections::HashSet<String> {
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

    let names = snapshot_process_names();
    *guard = Some((Instant::now(), names.clone()));
    names
}

/// Disclosed heuristic shared by providers that know a game's install
/// folder but not its exact main exe (Steam's manifest gives an installdir,
/// never a launch exe; native Ubisoft installs are the same) — prefers an
/// exe whose filename matches the folder name (the common convention for
/// single-exe games), falling back to the largest top-level .exe, skipping
/// known non-game utility exes. Not exact for every multi-exe install, but
/// it's still a real local file's real icon, not a guess at what the icon
/// looks like.
pub fn find_main_exe(dir: &std::path::Path) -> Option<std::path::PathBuf> {
    let folder_name = dir.file_name()?.to_str()?.to_ascii_lowercase();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return None;
    };
    const SKIP_PREFIXES: &[&str] = &["unins", "vc_redist", "dxwebsetup", "dotnetfx", "directx"];

    let mut candidates: Vec<(std::path::PathBuf, u64)> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()).map(|e| e.eq_ignore_ascii_case("exe")) != Some(true) {
            continue;
        }
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_ascii_lowercase();
        if SKIP_PREFIXES.iter().any(|p| stem.starts_with(p)) {
            continue;
        }
        if stem == folder_name {
            return Some(path);
        }
        let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
        candidates.push((path, size));
    }
    candidates.into_iter().max_by_key(|(_, size)| *size).map(|(path, _)| path)
}

/// True if `path` exists and was last modified within `window` of now — the
/// "is it actively streaming bytes right now" recency check shared by
/// epic.rs (a staging-folder file) and ea.rs (a per-component .tmp file),
/// each of which independently implemented this same predicate over their
/// own, differently-shaped directory scan.
pub fn is_recently_modified(path: &std::path::Path, window: std::time::Duration) -> bool {
    let now = std::time::SystemTime::now();
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .is_ok_and(|modified| now.duration_since(modified).is_ok_and(|age| age < window))
}

#[cfg(test)]
mod tests {
    use super::{drive_type_is_scannable, find_main_exe, spawn_exe, uri_safe_id};
    use std::fs;
    use std::path::PathBuf;

    /// A fresh, uniquely-named directory under the OS temp dir — no
    /// `tempfile` crate needed, `std::env::temp_dir()` plus a name unique to
    /// this test (and the process, so parallel `cargo test` runs never
    /// collide) is enough. Caller is responsible for removing it; tests here
    /// clean up at the end rather than via a drop guard, which is good
    /// enough for a local/CI-only test that isn't touching production paths.
    fn make_temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("launchpilot_test_{name}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn skips_unins_prefixed_exe_and_falls_back_to_largest() {
        let dir = make_temp_dir("skip_prefix");
        // Bigger than the real game exe, to prove the prefix skip actually
        // excludes it rather than it just losing on size.
        fs::write(dir.join("unins000.exe"), vec![0u8; 5000]).unwrap();
        fs::write(dir.join("MyGame.exe"), vec![0u8; 100]).unwrap();

        let result = find_main_exe(&dir);

        fs::remove_dir_all(&dir).unwrap();
        assert_eq!(result, Some(dir.join("MyGame.exe")));
    }

    #[test]
    fn prefers_exe_matching_folder_name_over_a_larger_file() {
        let dir = make_temp_dir("CoolGame");
        let folder_name = dir.file_name().unwrap().to_str().unwrap().to_string();
        fs::write(dir.join(format!("{folder_name}.exe")), vec![0u8; 10]).unwrap();
        fs::write(dir.join("other.exe"), vec![0u8; 99999]).unwrap();

        let result = find_main_exe(&dir);

        fs::remove_dir_all(&dir).unwrap();
        assert_eq!(result, Some(dir.join(format!("{folder_name}.exe"))));
    }

    #[test]
    fn falls_back_to_largest_exe_when_nothing_matches() {
        let dir = make_temp_dir("largest_fallback");
        fs::write(dir.join("small.exe"), vec![0u8; 10]).unwrap();
        fs::write(dir.join("biggest.exe"), vec![0u8; 500]).unwrap();
        fs::write(dir.join("medium.exe"), vec![0u8; 100]).unwrap();

        let result = find_main_exe(&dir);

        fs::remove_dir_all(&dir).unwrap();
        assert_eq!(result, Some(dir.join("biggest.exe")));
    }

    #[test]
    fn network_cdrom_and_missing_drives_are_not_scanned() {
        assert!(drive_type_is_scannable(3)); // FIXED
        assert!(drive_type_is_scannable(2)); // REMOVABLE
        assert!(!drive_type_is_scannable(4)); // REMOTE (mapped network drive)
        assert!(!drive_type_is_scannable(5)); // CDROM
        assert!(!drive_type_is_scannable(1)); // NO_ROOT_DIR
        assert!(!drive_type_is_scannable(0)); // UNKNOWN
    }

    #[test]
    fn uri_safe_id_accepts_real_id_shapes_and_rejects_the_rest() {
        for ok in ["730", "0449f415f5404df093b8d67c31940024", "FabPlugin_5.8", "steam-123"] {
            assert_eq!(uri_safe_id(ok), Ok(ok));
        }
        for bad in ["", ".", "..", "a b", "a/b", "a?x=1", "a#b", "a%20b", "a&b", "a\\b", "a\0b"] {
            assert!(uri_safe_id(bad).is_err(), "expected {bad:?} to be rejected");
        }
    }

    #[cfg(windows)]
    #[test]
    fn process_snapshot_contains_this_test_process() {
        let me = std::env::current_exe().unwrap().file_name().unwrap().to_string_lossy().to_ascii_lowercase();
        assert!(super::snapshot_process_names().contains(&me), "{me} not in snapshot");
    }

    #[test]
    fn spawn_exe_refuses_non_exe_targets() {
        let err = spawn_exe(std::path::Path::new("C:\\x\\game.ico"), None).unwrap_err();
        assert!(err.contains("not an executable"), "{err}");
    }

    #[test]
    fn empty_directory_returns_none() {
        let dir = make_temp_dir("empty_dir");

        let result = find_main_exe(&dir);

        fs::remove_dir_all(&dir).unwrap();
        assert_eq!(result, None);
    }
}

pub fn all_providers() -> Vec<Box<dyn LauncherProvider>> {
    vec![
        Box::new(steam::SteamProvider),
        Box::new(epic::EpicProvider),
        Box::new(gog::GogProvider),
        Box::new(ea::EaProvider),
        Box::new(ubisoft::UbisoftProvider),
        Box::new(battlenet::BattleNetProvider),
        Box::new(riot::RiotProvider),
        Box::new(prismlauncher::PrismLauncherProvider),
        Box::new(wargaming::WargamingProvider),
        Box::new(ageofthering::AgeOfTheRingProvider),
    ]
}
