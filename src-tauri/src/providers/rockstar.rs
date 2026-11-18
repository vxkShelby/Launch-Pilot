// Rockstar Games Launcher provider: detect + open only.
//
// Detection (read on the reference machine): HKLM\SOFTWARE\WOW6432Node\
// Rockstar Games\Launcher has `InstallFolder` (X:\RockstarLauncher) and
// `Version` (1.0.110.3150); `Launcher.exe` is in that folder and the same
// path is the Uninstall entry's DisplayIcon.
//
// Games: NOT listed. Looked for a readable list and found none:
//   - `%ProgramData%\Rockstar Games\Launcher\titles.dat` (6 KB) is
//     high-entropy binary with no readable structure (encrypted or
//     compressed), so no title list or version can be read from it.
//   - The only game key, `...\Rockstar Games\Grand Theft Auto V`, holds
//     `InstallFolderEpic`: an Epic-owned install the Epic provider already
//     lists, not a Rockstar Launcher install.
// Update status: no readable local field, so there are no game rows and
// nothing is reported as up to date or outdated.
//
// Launch: only the launcher itself can be opened (spawning its own
// Launcher.exe, same as the Riot provider). No per-game launch mechanism was
// verified, so `supports_launch` stays false.
use super::{Game, LauncherProvider};
use std::path::PathBuf;
use winreg::enums::HKEY_LOCAL_MACHINE;
use winreg::RegKey;

const LAUNCHER_KEY: &str = "SOFTWARE\\WOW6432Node\\Rockstar Games\\Launcher";

/// `InstallFolder` is only trusted when it is a non-empty absolute path.
fn launcher_exe_in(install_folder: &str) -> Option<PathBuf> {
    let folder = PathBuf::from(install_folder.trim());
    if install_folder.trim().is_empty() || !folder.is_absolute() {
        return None;
    }
    Some(folder.join("Launcher.exe"))
}

pub struct RockstarProvider;

impl RockstarProvider {
    fn exe(&self) -> Option<PathBuf> {
        let key = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(LAUNCHER_KEY).ok()?;
        let folder: String = key.get_value("InstallFolder").ok()?;
        launcher_exe_in(&folder)
    }
}

impl LauncherProvider for RockstarProvider {
    fn id(&self) -> &'static str {
        "rockstar"
    }

    fn display_name(&self) -> &'static str {
        "Rockstar Games Launcher"
    }

    fn detect(&self) -> bool {
        self.exe().is_some_and(|p| p.exists())
    }

    fn list_games(&self) -> Result<Vec<Game>, String> {
        Ok(Vec::new())
    }

    fn icon_source(&self) -> Option<PathBuf> {
        self.exe()
    }

    fn trigger_update(&self, game_id: &str) -> Result<(), String> {
        if !game_id.is_empty() {
            return Err("Rockstar Games Launcher can only be opened, not told to update a game".to_string());
        }
        let exe = self.exe().ok_or("Rockstar Games Launcher not installed")?;
        super::spawn_exe(&exe, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launcher_exe_is_joined_onto_the_install_folder() {
        // Value shape read from the registry on the reference machine.
        assert_eq!(launcher_exe_in("X:\\RockstarLauncher"), Some(PathBuf::from("X:\\RockstarLauncher").join("Launcher.exe")));
        assert_eq!(launcher_exe_in("  X:\\RockstarLauncher "), Some(PathBuf::from("X:\\RockstarLauncher").join("Launcher.exe")));
    }

    #[test]
    fn empty_or_relative_install_folder_is_rejected() {
        assert_eq!(launcher_exe_in(""), None);
        assert_eq!(launcher_exe_in("   "), None);
        assert_eq!(launcher_exe_in("RockstarLauncher"), None);
    }

    #[test]
    fn no_games_are_listed_and_per_game_update_is_refused() {
        assert!(RockstarProvider.list_games().unwrap().is_empty());
        assert!(RockstarProvider.trigger_update("gta5").is_err());
        assert!(!RockstarProvider.supports_launch());
    }
}
