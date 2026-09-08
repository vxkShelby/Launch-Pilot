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
// Update status: DisplayVersion is the *installed* version, not something
// we can compare against a "latest available" — there's no local field for
// that anywhere in either scheme. Honest Unknown, same as Epic.
use super::{Game, LauncherProvider, UpdateStatus};
use winreg::enums::HKEY_LOCAL_MACHINE;
use winreg::RegKey;

const UNINSTALL_KEYS: [&str; 2] = [
    "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
    "SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
];

pub struct EaProvider;

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

                games.push(Game {
                    launcher: "ea",
                    id: subkey_name.trim_matches(|c| c == '{' || c == '}').to_string(),
                    name,
                    installed_build: version,
                    status: UpdateStatus::Unknown,
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
