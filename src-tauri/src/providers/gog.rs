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
// Update status: REAL, live network check — no local field indicates
// whether a newer build exists, but GOG's own content-system API does:
// https://content-system.gog.com/products/<id>/os/windows/builds?generation=2
// returns every published Windows build for that product, newest first,
// each with a real build_id — no API key, no login, verified live against
// a real installed game on this machine (Celtic Kings - Rage of War,
// product 1207658762). The registry's own BUILDID value (also verified
// present) is the installed build; if it doesn't match the newest one from
// the API, an update is available. This is the same public endpoint GOG's
// own gogdl downloader and Heroic Games Launcher use. Best-effort: any
// network failure (offline, GOG API down, unpublished/delisted product)
// just falls back to honest Unknown rather than blocking the dashboard.
// trigger_update deep-links into Galaxy via its own registered
// goggalaxy:// protocol, focused on the game's page.
use super::{Game, LauncherProvider, UpdateStatus};
use serde::Deserialize;

pub struct GogProvider;

const GAMES_KEY: &str = "SOFTWARE\\WOW6432Node\\GOG.com\\Games";

#[derive(Deserialize)]
struct BuildsResponse {
    items: Vec<BuildItem>,
}

#[derive(Deserialize)]
struct BuildItem {
    build_id: String,
}

impl GogProvider {
    fn latest_build_id(product_id: &str) -> Option<String> {
        let url = format!("https://content-system.gog.com/products/{product_id}/os/windows/builds?generation=2");
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(4))
            .build()
            .ok()?;
        let resp: BuildsResponse = client.get(url).send().ok()?.json().ok()?;
        resp.items.into_iter().next().map(|b| b.build_id)
    }
}

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

        struct Entry {
            id: String,
            name: String,
            version: Option<String>,
            build_id: Option<String>,
        }

        let mut entries = Vec::new();
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
            let build_id: Option<String> = game_key.get_value("BUILDID").ok();
            entries.push(Entry { id, name, version, build_id });
        }

        // Each entry's latest-build lookup is its own network round-trip
        // (up to the 4s timeout each) — run them concurrently so N
        // installed GOG games cost one round-trip's worth of wall time
        // instead of stalling the whole dashboard refresh N-in-a-row.
        let games = std::thread::scope(|scope| {
            let handles: Vec<_> = entries
                .into_iter()
                .map(|entry| scope.spawn(move || {
                    let latest = Self::latest_build_id(&entry.id);
                    (entry, latest)
                }))
                .collect();
            handles
                .into_iter()
                .filter_map(|h| h.join().ok())
                .map(|(entry, latest)| {
                    let status = match (&entry.build_id, latest) {
                        (Some(installed), Some(latest)) if *installed != latest => UpdateStatus::UpdateAvailable,
                        (Some(_), Some(_)) => UpdateStatus::UpToDate,
                        _ => UpdateStatus::Unknown,
                    };
                    Game {
                        launcher: "gog",
                        id: entry.id,
                        name: entry.name,
                        installed_build: entry.version,
                        status,
                        size_bytes: None,
                        last_updated: None,
                    }
                })
                .collect()
        });
        Ok(games)
    }

    // NOT verified locally — GOG Galaxy client itself isn't installed on
    // this machine (only game registry entries are). CORROBORATED: the
    // client's own installer and multiple open-source tools that detect it
    // running (e.g. Heroic Games Launcher's Windows process checks) use
    // "GalaxyClient.exe" as its process/image name.
    fn process_names(&self) -> &'static [&'static str] {
        &["GalaxyClient.exe"]
    }

    fn trigger_update(&self, game_id: &str) -> Result<(), String> {
        open::that(format!("goggalaxy://openGameView/{game_id}")).map_err(|e| e.to_string())
    }
}
