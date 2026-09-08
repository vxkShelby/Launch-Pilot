// EA app (EA Desktop) provider.
//
// Detection: HKLM\SOFTWARE\WOW6432Node\Electronic Arts\EA Desktop\ClientPath,
// verified against a real local install.
// Enumeration: each installed EA title registers a subkey under
// HKLM\SOFTWARE\WOW6432Node\EA Games\<Name> with DisplayName/Install Dir/
// Product GUID — verified live (a real Battlefield 6 install produced
// exactly this shape). This is the same tier of confidence as the GOG
// registry approach.
// No update status: neither this key nor EA's local per-game state store
// (content.db / IGO db under %ProgramData%\EA Desktop, which is an opaque
// binary format that changed across EA app versions and wasn't safe to
// reverse-engineer) carries a version/build field — honest Unknown, same
// treatment as Epic.
// origin2:// is the real registered protocol (EA app still ships it for
// backwards compatibility) — verified: HKCR\origin2\shell\open\command
// points at EALauncher.exe on this machine.
use super::{Game, LauncherProvider, UpdateStatus};

const GAMES_KEY: &str = "SOFTWARE\\WOW6432Node\\EA Games";

pub struct EaProvider;

impl LauncherProvider for EaProvider {
    fn id(&self) -> &'static str {
        "ea"
    }

    fn display_name(&self) -> &'static str {
        "EA app"
    }

    fn detect(&self) -> bool {
        let hklm = winreg::RegKey::predef(winreg::enums::HKEY_LOCAL_MACHINE);
        hklm.open_subkey("SOFTWARE\\WOW6432Node\\Electronic Arts\\EA Desktop")
            .is_ok()
    }

    fn list_games(&self) -> Result<Vec<Game>, String> {
        let hklm = winreg::RegKey::predef(winreg::enums::HKEY_LOCAL_MACHINE);
        let Ok(games_key) = hklm.open_subkey(GAMES_KEY) else {
            return Ok(Vec::new());
        };

        let mut games = Vec::new();
        for subkey_name in games_key.enum_keys().flatten() {
            let Ok(game_key) = games_key.open_subkey(&subkey_name) else {
                continue;
            };
            let name: String = game_key.get_value("DisplayName").unwrap_or_else(|_| subkey_name.clone());
            let id: String = game_key
                .get_value("Product GUID")
                .unwrap_or_else(|_| subkey_name.clone());

            games.push(Game {
                launcher: "ea",
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
        open::that("origin2://").map_err(|e| e.to_string())
    }
}
