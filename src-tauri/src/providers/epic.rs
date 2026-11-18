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
        entries.flatten().any(|e| super::is_recently_modified(&e.path(), ACTIVE_WINDOW))
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
                ..Game::default()
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

    // Epic has no verified update-only trigger. The previous implementation
    // used `com.epicgames.launcher://apps/<id>?action=launch&silent=true`,
    // which is a *launch* action — it would have started the game under an
    // "update" label. Removed: a per-game id now returns an honest Err, and
    // `supports_update()` is false so the frontend does not offer the action.
    // The launcher-level call (empty id, the "Open" button) starts the Epic
    // client from the command registered for its own URI scheme (read from
    // HKCR\com.epicgames.launcher\shell\open\command — present on the
    // reference machine as `"<...>\EpicGamesLauncher.exe" %1`).
    //
    // Left unused on purpose: each .item's `PendingManifestPath`. Checked on
    // the reference machine: it is populated on every item, including ones
    // with nothing queued (all `bIsIncompleteInstall=false`), and the file it
    // points at does not exist — so it is not an "update pending" signal.
    fn trigger_update(&self, game_id: &str) -> Result<(), String> {
        if !game_id.is_empty() {
            return Err("Epic Games has no update-only trigger; open the Epic Games Launcher to update this game".to_string());
        }
        let exe = Self::client_exe().ok_or("Epic Games Launcher not found")?;
        super::spawn_exe(&exe, None)
    }

    fn supports_update(&self) -> bool {
        false
    }
}

impl EpicProvider {
    /// Client exe from the `com.epicgames.launcher` URI handler registration.
    fn client_exe() -> Option<PathBuf> {
        let hkcr = winreg::RegKey::predef(winreg::enums::HKEY_CLASSES_ROOT);
        let command: String = hkcr
            .open_subkey("com.epicgames.launcher\\shell\\open\\command")
            .ok()?
            .get_value("")
            .ok()?;
        exe_from_command(&command)
    }
}

/// First token of a registry shell command: the quoted path, or the text up
/// to the first space when unquoted.
fn exe_from_command(command: &str) -> Option<PathBuf> {
    let command = command.trim();
    let exe = if let Some(rest) = command.strip_prefix('"') {
        rest.split('"').next()?
    } else {
        command.split_whitespace().next()?
    };
    (!exe.is_empty()).then(|| PathBuf::from(exe))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exe_from_command_handles_quoted_and_unquoted() {
        // Shape read from this machine's registry.
        assert_eq!(
            exe_from_command("\"X:\\Epic Games\\Launcher\\Portal\\Binaries\\Win64\\EpicGamesLauncher.exe\" %1"),
            Some(PathBuf::from("X:\\Epic Games\\Launcher\\Portal\\Binaries\\Win64\\EpicGamesLauncher.exe"))
        );
        assert_eq!(exe_from_command("C:\\x\\a.exe %1"), Some(PathBuf::from("C:\\x\\a.exe")));
        assert_eq!(exe_from_command(""), None);
    }

    #[test]
    fn per_game_update_is_refused_instead_of_launching_the_game() {
        let err = EpicProvider.trigger_update("0449f415f5404df093b8d67c31940024").unwrap_err();
        assert!(err.contains("no update-only trigger"), "{err}");
        assert!(!EpicProvider.supports_update());
    }
}
