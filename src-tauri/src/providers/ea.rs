// EA app (EA Desktop) provider.
//
// Detection: HKLM\SOFTWARE\WOW6432Node\Electronic Arts\EA Desktop\ClientPath,
// verified against a real local install.
//
// Enumeration: the standard Windows uninstall registry
// (HKLM\...\CurrentVersion\Uninstall, both the native and WOW6432Node views),
// filtered to entries whose Publisher is EA. This replaced an earlier
// attempt that only read HKLM\SOFTWARE\WOW6432Node\EA Games\<Name> — that
// key turned out to only cover EA's newer per-title registration scheme.
// A real install of The Sims 4 on this machine registers instead under the
// legacy HKLM\SOFTWARE\WOW6432Node\Origin Games\<contentId> scheme, which
// has no usable per-entry fields (just a default value with the game name).
// The uninstall registry is what both schemes have in common — verified
// live: it lists Battlefield 6, The Sims 4, and the EA app launcher itself
// (excluded by name), each with a real DisplayVersion.
//
// Live update-in-progress detection: verified against a real, currently
// running Battlefield 6 update on this machine. EA Desktop stages package
// updates under %ProgramData%\EA Desktop\InstallData\<Game>\<component>\ —
// files at rest are .eacrc/.eacd/.eaa/.eajrn; a *.tmp file only appears in
// one of those component subfolders while a download/apply is active
// (confirmed: it was present mid-update, its containing folder pattern is
// `dlc-Origin.SFT.*` or `base-Origin.SFT.*`). The <Game> folder name is the
// DisplayName with trademark/registered symbols stripped — also verified
// live ("Battlefield 6" folder vs. "Battlefield™ 6" registry DisplayName).
// This ties the signal to a specific game via its own install folder,
// unlike EA's log file (EADesktop.log's installedStatus=[Active] lines are
// keyed by EA's internal baseSlug/softwareId, which has no verified mapping
// back to a DisplayName) — so the .tmp check is the one used here.
//
// Bug fix, verified live: the .tmp file's mere *presence* is not "active
// right now" — pausing a download in EA Desktop leaves the same partial
// .tmp file sitting on disk untouched, so the old presence-only check kept
// reporting "Updating" forever. Confirmed on this machine: pausing
// Battlefield 6's update left deps.tmp in place with its LastWriteTime
// frozen at the pause moment. Fixed by splitting the one real signal into
// two states instead of collapsing a paused download into "Unknown": a
// .tmp file written in the last 10 seconds means a download is actively
// streaming bytes right now (Updating); a .tmp file that exists but is
// stale means an update was started and is sitting there incomplete —
// paused or stalled, not finished — which is exactly what UpdateAvailable
// means elsewhere in this app (pending, not yet done).
//
// Update status otherwise: DisplayVersion is the *installed* version, not
// something we can compare against a "latest available" — there's no local
// field for that. Honest Unknown when no .tmp exists at all.
use super::{Game, LauncherProvider, UpdateStatus};
use winreg::enums::HKEY_LOCAL_MACHINE;
use winreg::RegKey;

const UNINSTALL_KEYS: [&str; 2] = [
    "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
    "SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
];

pub struct EaProvider;

impl EaProvider {
    /// Real local signal: a *.tmp staging file in the game's InstallData
    /// folder means a download/apply is pending. Whether it's actively
    /// streaming right now (Updating) or sitting there paused/stalled
    /// (UpdateAvailable) is told apart by how recently it was written to.
    fn update_status_from_tmp(display_name: &str) -> UpdateStatus {
        let Ok(program_data) = std::env::var("ProgramData") else {
            return UpdateStatus::Unknown;
        };
        let folder_name: String = display_name.chars().filter(|c| *c != '\u{2122}' && *c != '\u{00AE}').collect();
        let install_data = std::path::PathBuf::from(program_data)
            .join("EA Desktop\\InstallData")
            .join(folder_name.trim());

        let Ok(components) = std::fs::read_dir(&install_data) else {
            return UpdateStatus::Unknown;
        };
        const ACTIVE_WINDOW: std::time::Duration = std::time::Duration::from_secs(10);
        let now = std::time::SystemTime::now();

        let mut found_pending = false;
        for component in components.flatten() {
            let Ok(files) = std::fs::read_dir(component.path()) else {
                continue;
            };
            for f in files.flatten() {
                if f.path().extension().and_then(|e| e.to_str()) != Some("tmp") {
                    continue;
                }
                found_pending = true;
                let is_actively_written = f
                    .metadata()
                    .and_then(|m| m.modified())
                    .is_ok_and(|modified| now.duration_since(modified).is_ok_and(|age| age < ACTIVE_WINDOW));
                if is_actively_written {
                    return UpdateStatus::Updating;
                }
            }
        }
        if found_pending {
            UpdateStatus::UpdateAvailable
        } else {
            UpdateStatus::Unknown
        }
    }
}

impl LauncherProvider for EaProvider {
    fn id(&self) -> &'static str {
        "ea"
    }

    fn display_name(&self) -> &'static str {
        "EA app"
    }

    fn detect(&self) -> bool {
        let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
        hklm.open_subkey("SOFTWARE\\WOW6432Node\\Electronic Arts\\EA Desktop")
            .is_ok()
    }

    fn list_games(&self) -> Result<Vec<Game>, String> {
        let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
        let mut games = Vec::new();
        let mut seen_names = std::collections::HashSet::new();

        for uninstall_path in UNINSTALL_KEYS {
            let Ok(uninstall_key) = hklm.open_subkey(uninstall_path) else {
                continue;
            };
            for subkey_name in uninstall_key.enum_keys().flatten() {
                let Ok(entry) = uninstall_key.open_subkey(&subkey_name) else {
                    continue;
                };
                let publisher: String = entry.get_value("Publisher").unwrap_or_default();
                if !publisher.contains("Electronic Arts") {
                    continue;
                }
                let name: String = entry.get_value("DisplayName").unwrap_or_default();
                if name.is_empty() || name == "EA app" || !seen_names.insert(name.clone()) {
                    continue;
                }
                let version: Option<String> = entry.get_value("DisplayVersion").ok();
                let status = Self::update_status_from_tmp(&name);

                games.push(Game {
                    launcher: "ea",
                    id: subkey_name.trim_matches(|c| c == '{' || c == '}').to_string(),
                    name,
                    installed_build: version,
                    status,
                    size_bytes: None,
                    last_updated: None,
                });
            }
        }
        Ok(games)
    }

    fn trigger_update(&self, _game_id: &str) -> Result<(), String> {
        open::that("origin2://").map_err(|e| e.to_string())
    }
}
