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
use super::{Game, LauncherProvider, UpdateStatus};
use winreg::enums::HKEY_LOCAL_MACHINE;
use winreg::RegKey;

pub struct UbisoftProvider;

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
        Ok(games)
    }

    fn trigger_update(&self, _game_id: &str) -> Result<(), String> {
        open::that("uplay://").map_err(|e| e.to_string())
    }
}
