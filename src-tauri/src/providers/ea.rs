// EA app (EA Desktop) provider — detect + deep-link only.
//
// Detection: HKLM\SOFTWARE\WOW6432Node\Electronic Arts\EA Desktop\ClientPath,
// verified against a real local install.
// No enumeration: EA's local game-state store (content.db / IGO db under
// %ProgramData%\EA Desktop) is an undocumented internal format that has
// changed across EA app versions — enumerating it reliably isn't something
// we can verify right now, so per the project's honesty rule we don't
// pretend to. detect() + deep-link is the honest scope for this launcher.
// origin2:// is the real registered protocol (EA app still ships it for
// backwards compatibility) — verified: HKCR\origin2\shell\open\command
// points at EALauncher.exe on this machine.
use super::{Game, LauncherProvider};

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
        Ok(Vec::new())
    }

    fn trigger_update(&self, _game_id: &str) -> Result<(), String> {
        open::that("origin2://").map_err(|e| e.to_string())
    }
}
