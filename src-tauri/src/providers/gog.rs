// GOG Galaxy provider.
//
// Detection + enumeration: HKLM\SOFTWARE\WOW6432Node\GOG.com\Games, one
// subkey per installed game — this is written by GOG's own installers and
// is present even without Galaxy running, which is why we use it instead of
// parsing galaxy-2.0.db (that file only exists once Galaxy itself has
// touched the game, and even then only holds cover-art metadata, not
// install/version state — confirmed by reading real open-source code that
// consumes it: github.com/beeradmoore/dlss-swapper's GOGLibrary.cs).
// Verified against a real local install: gameID, gameName, path, ver,
// BUILDID, dependsOn (empty string for a base game, non-empty for DLC,
// which we skip) all present as expected.
//
// Update status: no local field indicates whether a newer build exists —
// honest Unknown. trigger_update deep-links into Galaxy via its own
// registered goggalaxy:// protocol, focused on the game's page.
use super::{Game, LauncherProvider, UpdateStatus};

pub struct GogProvider;

const GAMES_KEY: &str = "SOFTWARE\\WOW6432Node\\GOG.com\\Games";

impl LauncherProvider for GogProvider {
    fn id(&self) -> &'static str {
        "gog"
    }

    fn display_name(&self) -> &'static str {
        "GOG Galaxy"
    }

    fn detect(&self) -> bool {
        let hklm = winreg::RegKey::predef(winreg::enums::HKEY_LOCAL_MACHINE);
        hklm.open_subkey(GAMES_KEY).is_ok()
    }

    fn list_games(&self) -> Result<Vec<Game>, String> {
        let hklm = winreg::RegKey::predef(winreg::enums::HKEY_LOCAL_MACHINE);
        let games_key = hklm
            .open_subkey(GAMES_KEY)
            .map_err(|_| "GOG Galaxy not installed")?;

        let mut games = Vec::new();
        for subkey_name in games_key.enum_keys().flatten() {
            let Ok(game_key) = games_key.open_subkey(&subkey_name) else {
                continue;
            };

            let depends_on: String = game_key.get_value("dependsOn").unwrap_or_default();
            if !depends_on.is_empty() {
                continue; // DLC entry, not a standalone game
            }

            let Ok(id) = game_key.get_value::<String, _>("gameID") else {
                continue;
            };
            let Ok(name) = game_key.get_value::<String, _>("gameName") else {
                continue;
            };
            let version: Option<String> = game_key.get_value("ver").ok();

            games.push(Game {
                launcher: "gog",
                id,
                name,
                installed_build: version,
                status: UpdateStatus::Unknown,
                size_bytes: None,
                last_updated: None,
            });
        }
        Ok(games)
    }

    fn trigger_update(&self, game_id: &str) -> Result<(), String> {
        open::that(format!("goggalaxy://openGameView/{game_id}")).map_err(|e| e.to_string())
    }
}
