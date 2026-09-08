// Steam reference provider.
//
// Detection: SteamPath from HKCU\Software\Valve\Steam (written by the Steam
// client itself on every launch).
// Library discovery: <SteamPath>/steamapps/libraryfolders.vdf lists every
// library folder as a numbered block with a "path" key.
// Game enumeration: each library's steamapps/appmanifest_<appid>.acf holds
// one flat "AppState" block per installed game.
// Update detection: Steam's own "StateFlags" bitfield (values verified
// against a real install, not guessed):
//   1=Uninstalled 2=UpdateRequired 4=FullyInstalled 8=Encrypted 16=Locked
//   32=FilesMissing 64=AppRunning 128=FilesCorrupt 256=UpdateRunning
// A game also carries "buildid" (installed) vs "TargetBuildID" (what Steam
// wants installed next); TargetBuildID is "0" when Steam hasn't queued one.
use super::{Game, LauncherProvider, UpdateStatus};
use crate::vdf;
use std::path::{Path, PathBuf};

const STATE_UPDATE_REQUIRED: u64 = 2;
const STATE_FULLY_INSTALLED: u64 = 4;

pub struct SteamProvider;

impl SteamProvider {
    fn steam_path(&self) -> Option<PathBuf> {
        let hkcu = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER);
        let key = hkcu.open_subkey("Software\\Valve\\Steam").ok()?;
        let path: String = key.get_value("SteamPath").ok()?;
        Some(PathBuf::from(path.replace('/', "\\")))
    }

    fn library_paths(&self, steam_path: &Path) -> Vec<PathBuf> {
        let vdf_path = steam_path.join("steamapps").join("libraryfolders.vdf");
        let Ok(content) = std::fs::read_to_string(&vdf_path) else {
            return vec![steam_path.to_path_buf()];
        };
        let Some(root) = vdf::parse(&content) else {
            return vec![steam_path.to_path_buf()];
        };
        let Some(folders) = root.get("libraryfolders").and_then(|v| v.as_block()) else {
            return vec![steam_path.to_path_buf()];
        };

        let mut paths = Vec::new();
        for entry in folders.values() {
            if let Some(block) = entry.as_block() {
                if let Some(path) = block.get("path").and_then(|v| v.as_str()) {
                    paths.push(PathBuf::from(path.replace('\\', "/")));
                }
            }
        }
        if paths.is_empty() {
            paths.push(steam_path.to_path_buf());
        }
        paths
    }

    fn parse_manifest(&self, path: &Path) -> Option<Game> {
        let content = std::fs::read_to_string(path).ok()?;
        let root = vdf::parse(&content)?;
        let state = root.get("AppState")?.as_block()?;

        let id = state.get("appid")?.as_str()?.to_string();
        let name = state.get("name")?.as_str()?.to_string();
        let flags: u64 = state
            .get("StateFlags")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let buildid = state.get("buildid").and_then(|v| v.as_str());
        let target_buildid = state.get("TargetBuildID").and_then(|v| v.as_str());

        let status = if flags & STATE_UPDATE_REQUIRED != 0 {
            UpdateStatus::UpdateAvailable
        } else if let (Some(b), Some(t)) = (buildid, target_buildid) {
            if t != "0" && t != b {
                UpdateStatus::UpdateAvailable
            } else if flags & STATE_FULLY_INSTALLED != 0 {
                UpdateStatus::UpToDate
            } else {
                UpdateStatus::Unknown
            }
        } else if flags & STATE_FULLY_INSTALLED != 0 {
            UpdateStatus::UpToDate
        } else {
            UpdateStatus::Unknown
        };

        Some(Game {
            launcher: "steam",
            id,
            name,
            installed_build: buildid.map(String::from),
            status,
            size_bytes: state
                .get("SizeOnDisk")
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse().ok()),
            last_updated: state
                .get("LastUpdated")
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse().ok()),
        })
    }
}

impl LauncherProvider for SteamProvider {
    fn id(&self) -> &'static str {
        "steam"
    }

    fn display_name(&self) -> &'static str {
        "Steam"
    }

    fn detect(&self) -> bool {
        self.steam_path().is_some_and(|p| p.join("steam.exe").exists())
    }

    fn list_games(&self) -> Result<Vec<Game>, String> {
        let steam_path = self.steam_path().ok_or("Steam not installed")?;
        let mut games = Vec::new();

        for library in self.library_paths(&steam_path) {
            let steamapps = library.join("steamapps");
            let Ok(entries) = std::fs::read_dir(&steamapps) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                let is_manifest = path
                    .file_name()
                    .and_then(|f| f.to_str())
                    .is_some_and(|f| f.starts_with("appmanifest_") && f.ends_with(".acf"));
                if is_manifest {
                    if let Some(game) = self.parse_manifest(&path) {
                        games.push(game);
                    }
                }
            }
        }
        Ok(games)
    }

    fn process_names(&self) -> &'static [&'static str] {
        &["steam.exe"]
    }

    fn icon_source(&self) -> Option<PathBuf> {
        Some(self.steam_path()?.join("steam.exe"))
    }

    // Steam's own manifest gives an installdir but never a launch exe
    // (that lives in localconfig.vdf's launch options, a much messier
    // per-user file) — same disclosed "matching/largest exe" heuristic
    // used for Ubisoft, applied to the real install folder.
    fn game_icon_source(&self, game_id: &str) -> Option<PathBuf> {
        let steam_path = self.steam_path()?;
        for library in self.library_paths(&steam_path) {
            let manifest = library.join("steamapps").join(format!("appmanifest_{game_id}.acf"));
            let Ok(content) = std::fs::read_to_string(&manifest) else {
                continue;
            };
            let Some(root) = crate::vdf::parse(&content) else {
                continue;
            };
            let Some(installdir) = root
                .get("AppState")
                .and_then(|v| v.as_block())
                .and_then(|s| s.get("installdir"))
                .and_then(|v| v.as_str())
            else {
                continue;
            };
            let game_dir = library.join("steamapps").join("common").join(installdir);
            if let Some(exe) = super::find_main_exe(&game_dir) {
                return Some(exe);
            }
        }
        None
    }

    fn trigger_update(&self, game_id: &str) -> Result<(), String> {
        // No public "force update" API exists for Steam. steam://validate
        // is Valve's own documented mechanism for re-verifying a game's
        // files, which re-downloads anything missing or out of date.
        open::that(format!("steam://validate/{game_id}")).map_err(|e| e.to_string())
    }
}
