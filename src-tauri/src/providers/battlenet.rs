// Battle.net provider — detect + deep-link only.
//
// Detection: HKLM\SOFTWARE\WOW6432Node\Blizzard Entertainment\Battle.net\
// Capabilities, verified against a real local install.
// No enumeration: Battle.net's per-game/per-build state lives in its own
// internal Agent/product-db format that isn't documented and isn't verified
// here — honestly out of scope for now.
// battlenet:// is the real registered protocol — verified: HKCR\battlenet\
// shell\open\command runs Battle.net.exe --uri="%1" on this machine.
use super::{Game, LauncherProvider};

pub struct BattleNetProvider;

impl LauncherProvider for BattleNetProvider {
    fn id(&self) -> &'static str {
        "battlenet"
    }

    fn display_name(&self) -> &'static str {
        "Battle.net"
    }

    fn detect(&self) -> bool {
        let hklm = winreg::RegKey::predef(winreg::enums::HKEY_LOCAL_MACHINE);
        hklm.open_subkey("SOFTWARE\\WOW6432Node\\Blizzard Entertainment\\Battle.net\\Capabilities")
            .is_ok()
    }

    fn list_games(&self) -> Result<Vec<Game>, String> {
        Ok(Vec::new())
    }

    fn trigger_update(&self, _game_id: &str) -> Result<(), String> {
        open::that("battlenet://").map_err(|e| e.to_string())
    }
}
