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

#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum UpdateStatus {
    UpToDate,
    UpdateAvailable,
    /// A download/install is actively in progress right now (observed via
    /// the launcher's own live log/state, not inferred) — distinct from
    /// UpdateAvailable, which means "pending, not yet started."
    Updating,
    Unknown,
}

#[derive(Serialize, Clone)]
pub struct Game {
    pub launcher: &'static str,
    pub id: String,
    pub name: String,
    pub installed_build: Option<String>,
    pub status: UpdateStatus,
    pub size_bytes: Option<u64>,
    pub last_updated: Option<u64>,
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
    use super::find_main_exe;
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
