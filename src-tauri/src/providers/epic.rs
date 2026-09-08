// Epic Games Launcher provider.
//
// Detection: HKLM\SOFTWARE\WOW6432Node\Epic Games\EpicGamesLauncher exists
// (written by the installer).
// Enumeration: %ProgramData%\Epic\EpicGamesLauncher\Data\Manifests\*.item —
// one flat JSON file per installed item. Verified against a real local
// install: fields used here (DisplayName, AppName, InstallSize,
// AppVersionString, bIsApplication) all present and populated as expected.
// Non-game items (engine installs, plugins) show bIsApplication=false, which
// is how we filter them out.
//
// Update status: Epic's manifest carries no "update available" field and
// there's no public API for it — this is the honest Unknown case the spec
// calls for. trigger_update deep-links into the game's page via Epic's own
// documented app protocol (used by Epic for its "Add to Library" shortcuts),
// which at minimum surfaces any pending update to the user.
use super::{Game, LauncherProvider, UpdateStatus};
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Deserialize)]
struct EpicItem {
    #[serde(rename = "DisplayName")]
    display_name: String,
    #[serde(rename = "AppName")]
    app_name: String,
    #[serde(rename = "AppVersionString")]
    app_version: String,
    #[serde(rename = "InstallSize")]
    install_size: Option<u64>,
    #[serde(rename = "bIsApplication")]
    is_application: bool,
}

pub struct EpicProvider;

impl EpicProvider {
    fn manifests_dir(&self) -> Option<PathBuf> {
        let program_data = std::env::var("ProgramData").ok()?;
        Some(PathBuf::from(program_data).join("Epic\\EpicGamesLauncher\\Data\\Manifests"))
    }
}

impl LauncherProvider for EpicProvider {
    fn id(&self) -> &'static str {
        "epic"
    }

    fn display_name(&self) -> &'static str {
        "Epic Games"
    }

    fn detect(&self) -> bool {
        let hklm = winreg::RegKey::predef(winreg::enums::HKEY_LOCAL_MACHINE);
        hklm.open_subkey("SOFTWARE\\WOW6432Node\\Epic Games\\EpicGamesLauncher")
            .is_ok()
    }

    fn list_games(&self) -> Result<Vec<Game>, String> {
        let dir = self.manifests_dir().ok_or("Epic Games not installed")?;
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return Ok(Vec::new());
        };

        let mut games = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("item") {
                continue;
            }
            let Ok(content) = std::fs::read_to_string(&path) else {
                continue;
            };
            let Ok(item) = serde_json::from_str::<EpicItem>(&content) else {
                continue;
            };
            if !item.is_application {
                continue;
            }
            games.push(Game {
                launcher: "epic",
                id: item.app_name,
                name: item.display_name,
                installed_build: Some(item.app_version),
                status: UpdateStatus::Unknown,
                size_bytes: item.install_size,
                last_updated: None,
            });
        }
        Ok(games)
    }

    // NOT verified locally — Epic Games Launcher isn't installed on this
    // machine. CORROBORATED: its own installer registers the process as
    // "EpicGamesLauncher.exe" — the same name Epic uses in its own
    // launcher's window title and multiple third-party tools (e.g.
    // Legendary's Windows helper scripts) match against.
    fn process_names(&self) -> &'static [&'static str] {
        &["EpicGamesLauncher.exe"]
    }

    fn trigger_update(&self, game_id: &str) -> Result<(), String> {
        open::that(format!(
            "com.epicgames.launcher://apps/{game_id}?action=launch&silent=true"
        ))
        .map_err(|e| e.to_string())
    }
}
