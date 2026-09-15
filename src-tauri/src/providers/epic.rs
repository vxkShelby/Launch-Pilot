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
// Live update-in-progress detection: REAL field, verified live on this
// machine — every .item file (checked all 3 present here: an Unreal Engine
// install plus its Fab/Quixel plugin components) carries a top-level
// `bIsIncompleteInstall` boolean, currently false on all of them (nothing
// mid-download right now). This is Epic's own explicit flag, not an
// inferred one — independently corroborated by a third-party writeup of the
// same .item schema (jayd.ml/games/2020/05/16/epic-games-store-steam-libraries.html),
// which documents both `bIsIncompleteInstall` and `StagingLocation` (the
// folder Epic stages an active download's chunks into — verified live here
// too, e.g. "V:\UE_5.8\.egstore/bps").
//
// Same caveat the EA provider already hit and fixed (see ea.rs): the flag
// alone only means an install/update was started and hasn't finished —
// that covers a paused or abandoned download too, not just one actively
// streaming bytes right now. No installed item here has ever been left
// mid-download, so that distinction can't be observed live end-to-end; the
// same recency check ea.rs verified live is applied defensively — only a
// StagingLocation with a file written in the last few seconds counts as
// Updating. bIsIncompleteInstall=true with no recent write stays Unknown
// rather than guessing at a pending-but-not-active state (no evidence Epic's
// schema distinguishes one), which also keeps this within the task's
// Updating-only scope, not a fabricated UpdateAvailable.
//
// Update status otherwise: no field gives "latest available version" for a
// specific game — honest Unknown, same as before.
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
    // Both defaulted rather than required: only the 3 manifests on this
    // machine were verified to carry them (CORROBORATED elsewhere, not an
    // official Epic schema doc) — an older/different manifest missing either
    // field should fall back to the prior "Unknown, still listed" behavior,
    // not silently drop the whole game from the list.
    #[serde(rename = "bIsIncompleteInstall", default)]
    is_incomplete_install: bool,
    #[serde(rename = "StagingLocation", default)]
    staging_location: String,
}

pub struct EpicProvider;

impl EpicProvider {
    fn manifests_dir(&self) -> Option<PathBuf> {
        let program_data = std::env::var("ProgramData").ok()?;
        Some(PathBuf::from(program_data).join("Epic\\EpicGamesLauncher\\Data\\Manifests"))
    }

    /// True if any file directly under `staging_location` was written in the
    /// last few seconds — the same "is it actively streaming bytes right
    /// now" recency check ea.rs uses for its .tmp staging files, applied
    /// here since Epic's own bIsIncompleteInstall flag doesn't by itself
    /// distinguish "downloading right now" from "started once, now stalled."
    fn is_actively_staging(staging_location: &str) -> bool {
        if staging_location.is_empty() {
            return false;
        }
        let Ok(entries) = std::fs::read_dir(staging_location) else {
            return false;
        };
        const ACTIVE_WINDOW: std::time::Duration = std::time::Duration::from_secs(10);
        let now = std::time::SystemTime::now();
        entries.flatten().any(|e| {
            e.metadata()
                .and_then(|m| m.modified())
                .is_ok_and(|modified| now.duration_since(modified).is_ok_and(|age| age < ACTIVE_WINDOW))
        })
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
            let status = if item.is_incomplete_install && Self::is_actively_staging(&item.staging_location) {
                UpdateStatus::Updating
            } else {
                UpdateStatus::Unknown
            };
            games.push(Game {
                launcher: "epic",
                id: item.app_name,
                name: item.display_name,
                installed_build: Some(item.app_version),
                status,
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
