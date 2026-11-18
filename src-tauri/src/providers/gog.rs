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

/// Standalone so build-id comparison across all four `Option` combinations
/// can be unit-tested directly, without a real registry entry or network
/// round-trip.
fn derive_status(installed: Option<&str>, latest: Option<&str>) -> UpdateStatus {
    match (installed, latest) {
        (Some(i), Some(l)) if i != l => UpdateStatus::UpdateAvailable,
        (Some(_), Some(_)) => UpdateStatus::UpToDate,
        _ => UpdateStatus::Unknown,
    }
}

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

/// At most this many build lookups in flight at once. They all go to one
/// host, so an unbounded thread-per-game burst only added TLS handshakes.
const MAX_CONCURRENT_LOOKUPS: usize = 8;

/// GOG's `INSTALLDATE` value (`2025-10-20 22:19:49`) passed through only if
/// it has exactly that shape. It is machine-local wall-clock time with no
/// timezone: on the reference machine it reads ~1 minute after the game
/// folder's creation time shown in local time, not UTC. Hence it is exposed
/// as a string, not converted to an epoch.
fn valid_install_date(s: &str) -> Option<String> {
    let b = s.trim().as_bytes();
    let shape = b"dddd-dd-dd dd:dd:dd";
    let ok = b.len() == shape.len()
        && b.iter().zip(shape).all(|(c, p)| if *p == b'd' { c.is_ascii_digit() } else { c == p });
    ok.then(|| String::from_utf8_lossy(b).into_owned())
}

impl GogProvider {
    fn latest_build_id(client: &reqwest::blocking::Client, product_id: &str) -> Option<String> {
        super::uri_safe_id(product_id).ok()?; // id goes into the URL path
        let url = format!("https://content-system.gog.com/products/{product_id}/os/windows/builds?generation=2");
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
            install_date: Option<String>,
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

            // Real bug report: a game the user removed still showed up here.
            // GOG's own uninstaller is supposed to delete this whole subkey,
            // but doesn't always (a manually-deleted install folder, or an
            // interrupted uninstall, leaves the registry key behind) — this
            // key alone was never proof the game is still actually there.
            // `path` is the same real per-game field `game_icon_source`
            // already trusts for the exe; skip the entry if that folder is
            // gone rather than reporting an install that no longer exists.
            let path: String = game_key.get_value("path").unwrap_or_default();
            if !path.is_empty() && !std::path::Path::new(&path).exists() {
                continue;
            }

            let version: Option<String> = game_key.get_value("ver").ok();
            let build_id: Option<String> = game_key.get_value("BUILDID").ok();
            let install_date = game_key.get_value::<String, _>("INSTALLDATE").ok().and_then(|s| valid_install_date(&s));
            entries.push(Entry { id, name, version, build_id, install_date });
        }

        // Each entry's latest-build lookup is its own network round-trip
        // (up to the 4s timeout), so they run concurrently — but in bounded
        // batches over ONE shared client (one TLS config + connection pool)
        // rather than a thread and a fresh client per game.
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(4))
            .build()
            .ok();
        let mut latest: Vec<Option<String>> = Vec::with_capacity(entries.len());
        for batch in entries.chunks(MAX_CONCURRENT_LOOKUPS) {
            let results: Vec<Option<String>> = std::thread::scope(|scope| {
                let handles: Vec<_> = batch
                    .iter()
                    .map(|entry| {
                        let client = client.as_ref();
                        scope.spawn(move || client.and_then(|c| Self::latest_build_id(c, &entry.id)))
                    })
                    .collect();
                // A panicked lookup becomes "unknown", never a missing game.
                handles.into_iter().map(|h| h.join().unwrap_or(None)).collect()
            });
            latest.extend(results);
        }

        let games = entries
            .into_iter()
            .zip(latest)
            .map(|(entry, latest)| Game {
                launcher: "gog",
                status: derive_status(entry.build_id.as_deref(), latest.as_deref()),
                id: entry.id,
                name: entry.name,
                installed_build: entry.version,
                install_date: entry.install_date,
                ..Game::default()
            })
            .collect();
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

    // Real, verified live: HKLM\...\GOG.com\GalaxyClient\paths\client (the
    // install dir, "X:\GOG Galaxy") plus \GalaxyClient\clientExecutable
    // ("GalaxyClient.exe") combine into the real exe path — confirmed to
    // exist on disk on this machine. detect()/list_games() intentionally
    // don't rely on this key (it's separate from the per-game Games key
    // they use, and this project's own docs note the Galaxy client itself
    // wasn't otherwise verified installed) — but the registry key is real
    // regardless, so it's fine to use just for the icon.
    fn icon_source(&self) -> Option<std::path::PathBuf> {
        let hklm = winreg::RegKey::predef(winreg::enums::HKEY_LOCAL_MACHINE);
        let key = hklm.open_subkey("SOFTWARE\\WOW6432Node\\GOG.com\\GalaxyClient").ok()?;
        let exe_name: String = key.get_value("clientExecutable").ok()?;
        let client_dir: String = key.open_subkey("paths").ok()?.get_value("client").ok()?;
        Some(std::path::PathBuf::from(client_dir).join(exe_name))
    }

    fn trigger_update(&self, game_id: &str) -> Result<(), String> {
        // Empty id = launcher-level "Open": start the Galaxy client itself
        // (path from the registry value `icon_source` already uses).
        if game_id.is_empty() {
            let exe = self.icon_source().ok_or("GOG Galaxy client not found")?;
            return super::spawn_exe(&exe, None);
        }
        let id = super::uri_safe_id(game_id)?;
        open::that(format!("goggalaxy://openGameView/{id}")).map_err(|e| e.to_string())
    }

    // GOG's own per-game registry entry has a real, exact `exe` value
    // (verified live: "X:\GOG Galaxy\Games\Celtic Kings\Celtic Kings.exe")
    // — no heuristic needed, unlike providers that only know an install
    // folder.
    // Same staleness class `list_games` already guards against for the
    // install `path` value: a registry `exe` value can outlive the actual
    // file (interrupted uninstall, manually deleted install). A stale icon
    // path is harmless on its own (extract_icon_data_uri already checks
    // existence), but this path is also what `launch()` spawns — checking
    // here means a stale entry fails the same honest way in both places
    // instead of relying on Command::spawn's own error.
    fn game_icon_source(&self, game_id: &str) -> Option<std::path::PathBuf> {
        let hklm = winreg::RegKey::predef(winreg::enums::HKEY_LOCAL_MACHINE);
        let exe: String = hklm.open_subkey(GAMES_KEY).ok()?.open_subkey(game_id).ok()?.get_value("exe").ok()?;
        let path = std::path::PathBuf::from(exe);
        if !path.exists() {
            return None;
        }
        Some(path)
    }

    // Same real exe path already resolved for the game's icon — no separate
    // launch-target heuristic.
    // Working directory: the registry's own `workingDir` value (present next
    // to `exe`; "X:\GOG Galaxy\Games\Celtic Kings" on the reference machine),
    // falling back to the exe's folder.
    fn launch(&self, game_id: &str) -> Result<(), String> {
        let path = self.game_icon_source(game_id).ok_or("no verified exe path for this game")?;
        let hklm = winreg::RegKey::predef(winreg::enums::HKEY_LOCAL_MACHINE);
        let working_dir: Option<String> = hklm
            .open_subkey(GAMES_KEY)
            .ok()
            .and_then(|k| k.open_subkey(game_id).ok())
            .and_then(|k| k.get_value("workingDir").ok());
        super::spawn_exe(&path, working_dir.as_deref().filter(|d| !d.is_empty()).map(std::path::Path::new))
    }

    fn supports_launch(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn different_build_ids_report_update_available() {
        assert_eq!(derive_status(Some("100"), Some("101")), UpdateStatus::UpdateAvailable);
    }

    #[test]
    fn equal_build_ids_report_up_to_date() {
        assert_eq!(derive_status(Some("100"), Some("100")), UpdateStatus::UpToDate);
    }

    #[test]
    fn missing_installed_build_is_unknown() {
        assert_eq!(derive_status(None, Some("101")), UpdateStatus::Unknown);
    }

    #[test]
    fn missing_latest_build_is_unknown() {
        assert_eq!(derive_status(Some("100"), None), UpdateStatus::Unknown);
    }

    #[test]
    fn both_missing_is_unknown() {
        assert_eq!(derive_status(None, None), UpdateStatus::Unknown);
    }

    #[test]
    fn install_date_passes_only_the_observed_shape() {
        assert_eq!(valid_install_date("2025-10-20 22:19:49"), Some("2025-10-20 22:19:49".to_string()));
        assert_eq!(valid_install_date(" 2025-10-20 22:19:49 "), Some("2025-10-20 22:19:49".to_string()));
        for bad in ["", "2025-10-20", "20.10.2025 22:19:49", "2025-10-20T22:19:49", "abcd-ef-gh ii:jj:kk"] {
            assert_eq!(valid_install_date(bad), None, "{bad:?}");
        }
    }
}
