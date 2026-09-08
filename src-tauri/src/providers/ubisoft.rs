// Ubisoft Connect provider.
//
// Detection: HKLM\SOFTWARE\WOW6432Node\Ubisoft\Launcher\InstallDir, verified
// against a real local install.
//
// Enumeration: no Ubisoft game is installed on this machine to verify
// end-to-end, so this is CORROBORATED via real third-party open-source code
// — Playnite's UplayLibrary (github.com/JosefNemec/PlayniteExtensions,
// source/Libraries/UplayLibrary/Uplay.cs) reads exactly
// HKLM\SOFTWARE\ubisoft\Launcher\Installs\<id>\InstallDir per installed
// game, the same registry shape this file's detect() already confirmed
// exists (empty, but present, on this machine). Playnite additionally
// resolves display names from a protobuf-framed YAML cache
// (cache\configuration\configurations); that byte layout was only observed
// once by hand here, not confirmed stable/documented enough to parse with
// confidence, so this provider takes the more honest simpler path: the
// game's own install folder name as the display name (e.g. InstallDir
// `...\Assassins Creed Valhalla` -> "Assassins Creed Valhalla"). Less
// polished than the catalog title, but it's real data, not a guess.
//
// Update status: no local field indicates a pending/active update for a
// specific game — honest Unknown, same as Epic/GOG.
//
// Steam-installed Ubisoft games: REAL, verified live on this machine.
// Several Ubisoft-published games bought on Steam (Far Cry 3, Rainbow Six
// Siege, Riders Republic, The Settlers: New Allies, Avatar: Frontiers of
// Pandora) never populate this file's own HKLM\...\Installs\<id>\InstallDir
// key — Steam runs Ubisoft's installer silently via its own InstallScript
// mechanism, which instead writes to HKCU\SOFTWARE\Ubisoft\Launcher\Installs
// (verified populated on this machine, 8 entries, InstallState=1 each) with
// no InstallDir or name field at all — so that HKCU key alone can't name a
// game either. What every one of those five real installs DOES ship, inside
// its own Steam install folder, is a real `UbisoftConnectInstaller.exe` (and
// often a matching `.vdf` install-script) — confirmed present on this
// machine in each of the five folders. So instead of the unusable HKCU id,
// this scans Steam's own library folders (same libraryfolders.vdf +
// appmanifest_*.acf format steam.rs already parses) for that installer
// file's presence, and reports the game using Steam's own verified name.
use super::{Game, LauncherProvider, UpdateStatus};
use crate::vdf;
use std::path::{Path, PathBuf};
use winreg::enums::HKEY_LOCAL_MACHINE;
use winreg::RegKey;

pub struct UbisoftProvider;

impl UbisoftProvider {
    fn steam_path() -> Option<PathBuf> {
        let hkcu = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER);
        let key = hkcu.open_subkey("Software\\Valve\\Steam").ok()?;
        let path: String = key.get_value("SteamPath").ok()?;
        Some(PathBuf::from(path.replace('/', "\\")))
    }

    fn library_paths(steam_path: &Path) -> Vec<PathBuf> {
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

    /// Steam-installed games that ship Ubisoft's own installer in their
    /// install folder — the real signal a Steam copy also needs Ubisoft
    /// Connect, since Ubisoft's own registry doesn't name these installs.
    fn steam_ubisoft_games() -> Vec<Game> {
        let Some(steam_path) = Self::steam_path() else {
            return Vec::new();
        };
        let mut games = Vec::new();
        for library in Self::library_paths(&steam_path) {
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
                if !is_manifest {
                    continue;
                }
                let Ok(content) = std::fs::read_to_string(&path) else {
                    continue;
                };
                let Some(root) = vdf::parse(&content) else {
                    continue;
                };
                let Some(state) = root.get("AppState").and_then(|v| v.as_block()) else {
                    continue;
                };
                let (Some(appid), Some(name), Some(installdir)) = (
                    state.get("appid").and_then(|v| v.as_str()),
                    state.get("name").and_then(|v| v.as_str()),
                    state.get("installdir").and_then(|v| v.as_str()),
                ) else {
                    continue;
                };
                let game_dir = steamapps.join("common").join(installdir);
                let has_ubisoft_installer = game_dir.join("UbisoftConnectInstaller.exe").exists()
                    || game_dir.join("UbisoftConnectInstaller.vdf").exists();
                if !has_ubisoft_installer {
                    continue;
                }
                games.push(Game {
                    launcher: "ubisoft",
                    id: format!("steam-{appid}"),
                    name: name.to_string(),
                    installed_build: None,
                    status: UpdateStatus::Unknown,
                    size_bytes: None,
                    last_updated: None,
                });
            }
        }
        games
    }

    /// Re-finds a Steam-installed game's own install folder by appid, for
    /// icon lookup — same library-scan logic as `steam_ubisoft_games`,
    /// factored out since icon lookup only needs the one game, not a list.
    fn steam_game_dir(appid: &str) -> Option<PathBuf> {
        let steam_path = Self::steam_path()?;
        for library in Self::library_paths(&steam_path) {
            let manifest = library.join("steamapps").join(format!("appmanifest_{appid}.acf"));
            let Ok(content) = std::fs::read_to_string(&manifest) else {
                continue;
            };
            let Some(root) = vdf::parse(&content) else {
                continue;
            };
            let Some(installdir) = root.get("AppState").and_then(|v| v.as_block()).and_then(|s| s.get("installdir")).and_then(|v| v.as_str()) else {
                continue;
            };
            return Some(library.join("steamapps").join("common").join(installdir));
        }
        None
    }
}

impl LauncherProvider for UbisoftProvider {
    fn id(&self) -> &'static str {
        "ubisoft"
    }

    fn display_name(&self) -> &'static str {
        "Ubisoft Connect"
    }

    fn detect(&self) -> bool {
        let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
        hklm.open_subkey("SOFTWARE\\WOW6432Node\\Ubisoft\\Launcher")
            .is_ok()
    }

    fn list_games(&self) -> Result<Vec<Game>, String> {
        let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
        let Ok(installs_key) = hklm.open_subkey("SOFTWARE\\WOW6432Node\\Ubisoft\\Launcher\\Installs") else {
            return Ok(Vec::new());
        };

        let mut games = Vec::new();
        for id in installs_key.enum_keys().flatten() {
            let Ok(game_key) = installs_key.open_subkey(&id) else {
                continue;
            };
            let Ok(install_dir) = game_key.get_value::<String, _>("InstallDir") else {
                continue;
            };
            let name = std::path::Path::new(&install_dir)
                .file_name()
                .and_then(|f| f.to_str())
                .unwrap_or(&id)
                .to_string();

            games.push(Game {
                launcher: "ubisoft",
                id,
                name,
                installed_build: None,
                status: UpdateStatus::Unknown,
                size_bytes: None,
                last_updated: None,
            });
        }

        let mut seen_names: std::collections::HashSet<String> =
            games.iter().map(|g| g.name.clone()).collect();
        for game in Self::steam_ubisoft_games() {
            if seen_names.insert(game.name.clone()) {
                games.push(game);
            }
        }
        Ok(games)
    }

    // Verified live: this machine's install folder (from the registry's own
    // InstallDir) has UbisoftConnect.exe as the current main executable
    // (upc.exe also present, the older/legacy name for the same client).
    fn process_names(&self) -> &'static [&'static str] {
        &["UbisoftConnect.exe", "upc.exe"]
    }

    fn icon_source(&self) -> Option<PathBuf> {
        let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
        let install_dir: String = hklm
            .open_subkey("SOFTWARE\\WOW6432Node\\Ubisoft\\Launcher")
            .ok()?
            .get_value("InstallDir")
            .ok()?;
        Some(PathBuf::from(install_dir).join("UbisoftConnect.exe"))
    }

    // Same disclosed "largest/matching exe in the install folder" heuristic
    // as Steam, since neither native Ubisoft installs (InstallDir only) nor
    // the Steam-cross-detected ones (installdir from Steam's own manifest)
    // give an exact exe filename the way GOG's/EA's registries do.
    fn game_icon_source(&self, game_id: &str) -> Option<PathBuf> {
        if let Some(appid) = game_id.strip_prefix("steam-") {
            return super::find_main_exe(&Self::steam_game_dir(appid)?);
        }
        let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
        let install_dir: String = hklm
            .open_subkey("SOFTWARE\\WOW6432Node\\Ubisoft\\Launcher\\Installs")
            .ok()?
            .open_subkey(game_id)
            .ok()?
            .get_value("InstallDir")
            .ok()?;
        super::find_main_exe(&PathBuf::from(install_dir))
    }

    fn trigger_update(&self, _game_id: &str) -> Result<(), String> {
        open::that("uplay://").map_err(|e| e.to_string())
    }
}
