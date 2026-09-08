// Ubisoft Connect provider — detect + deep-link only.
//
// Detection: HKLM\SOFTWARE\WOW6432Node\Ubisoft\Launcher\InstallDir, verified
// against a real local install.
// No enumeration: Ubisoft Connect's per-game state lives in an internal
// SQLite/YAML combo under %LOCALAPPDATA%\Ubisoft Game Launcher whose schema
// isn't documented and isn't verified here — honestly out of scope for now.
// uplay:// is the real registered protocol — verified: HKCR\uplay\shell\
// open\command points at UbisoftConnect.exe on this machine.
use super::{Game, LauncherProvider};

pub struct UbisoftProvider;

impl LauncherProvider for UbisoftProvider {
    fn id(&self) -> &'static str {
        "ubisoft"
    }

    fn display_name(&self) -> &'static str {
        "Ubisoft Connect"
    }

    fn detect(&self) -> bool {
        let hklm = winreg::RegKey::predef(winreg::enums::HKEY_LOCAL_MACHINE);
        hklm.open_subkey("SOFTWARE\\WOW6432Node\\Ubisoft\\Launcher")
            .is_ok()
    }

    fn list_games(&self) -> Result<Vec<Game>, String> {
        Ok(Vec::new())
    }

    fn trigger_update(&self, _game_id: &str) -> Result<(), String> {
        open::that("uplay://").map_err(|e| e.to_string())
    }
}
