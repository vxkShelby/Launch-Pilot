// Battle.net provider.
//
// Detection: HKLM\SOFTWARE\WOW6432Node\Blizzard Entertainment\Battle.net\
// Capabilities, verified against a real local install.
//
// Enumeration: no Blizzard game is installed on this machine, so this is
// CORROBORATED via real third-party open-source code rather than locally
// verified end-to-end — Playnite's BattleNetLibrary (github.com/JosefNemec/
// PlayniteExtensions, source/Libraries/BattleNetLibrary) does NOT parse
// Blizzard's internal Agent/product-db format. It scans the standard
// Windows uninstall registry for entries whose UninstallString matches
// `Battle\.net.*--uid=(.*?)\s`, then maps the captured product uid against
// a small static table of known Blizzard game ids. Same technique this
// file's own EA provider already uses for a different publisher — no new
// pattern, just applied here per Playnite's confirmed regex.
//
// Live update-in-progress detection: REAL, verified live on this machine —
// %LOCALAPPDATA%\Battle.net\Logs\battle.net-<timestamp>.log contains lines
// like `[InstallManager] Operation status changed: opType=Update
// oldStatus=Off newStatus=On agentUid=<product>` (observed during the
// client's own self-update). newStatus=On for a matching agentUid means
// actively updating right now.
//
// Update status otherwise: no local field gives "latest available version"
// for a specific game — honest Unknown when nothing is actively updating.
use super::{Game, LauncherProvider, UpdateStatus};
use winreg::enums::HKEY_LOCAL_MACHINE;
use winreg::RegKey;

const UNINSTALL_KEYS: [&str; 2] = [
    "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
    "SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
];

// Known Blizzard product uid -> display name, per Playnite's BattleNetLibrary.
const KNOWN_PRODUCTS: &[(&str, &str)] = &[
    ("wow", "World of Warcraft"),
    ("wow_classic", "World of Warcraft Classic"),
    ("d3", "Diablo III"),
    ("d4", "Diablo IV"),
    ("fenris", "Diablo IV"),
    ("s1", "StarCraft"),
    ("s2", "StarCraft II"),
    ("prometheus", "Overwatch 2"),
    ("hero", "Heroes of the Storm"),
    ("hsb", "Hearthstone"),
    ("wlby", "Crash Bandicoot 4"),
    ("viper", "Call of Duty"),
    ("odin", "Call of Duty: Modern Warfare"),
    ("zeus", "Call of Duty: Black Ops Cold War"),
    ("fore", "Call of Duty: Vanguard"),
    ("lazr", "Call of Duty: Modern Warfare II"),
];

pub struct BattleNetProvider;

impl BattleNetProvider {
    fn logs_dir() -> Option<std::path::PathBuf> {
        let local_appdata = std::env::var("LOCALAPPDATA").ok()?;
        Some(std::path::PathBuf::from(local_appdata).join("Battle.net\\Logs"))
    }

    /// Reads the newest log file's full contents once. Returns None if the
    /// logs directory, newest file, or its contents can't be found/read —
    /// callers fall back to the same honest Unknown a per-game read failure
    /// used to produce.
    ///
    /// Previously this read (and rescanned) the newest log file separately
    /// for every installed game via a since-removed `is_updating(uid)`
    /// method — for N games that meant N re-reads of the same (potentially
    /// large) file in one `list_games()` call. Read once, reuse for every
    /// game's lookup instead.
    ///
    /// Note: despite the old comment claiming this "only scans the newest
    /// file's tail", the code has always read the newest file's entire
    /// contents into a String — this doc now matches that actual behavior.
    fn latest_log_content() -> Option<String> {
        let dir = Self::logs_dir()?;
        let entries = std::fs::read_dir(&dir).ok()?;
        let latest = entries
            .flatten()
            .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("log"))
            .max_by_key(|e| e.metadata().and_then(|m| m.modified()).ok())?;
        std::fs::read_to_string(latest.path()).ok()
    }

    /// True if the most recent line for this agent_uid, within already-read
    /// log content, shows an update currently running. Same matching logic
    /// the old per-game `is_updating` used (substring marker, most recent
    /// matching line wins) — now runs in memory against content read once
    /// per `list_games()` call instead of once per game.
    fn is_updating(content: &str, agent_uid: &str) -> bool {
        let marker = format!("agentUid={agent_uid}");
        content
            .lines()
            .rev()
            .find(|line| line.contains("[InstallManager] Operation status changed") && line.contains(&marker))
            .is_some_and(|line| line.contains("newStatus=On"))
    }
}

impl LauncherProvider for BattleNetProvider {
    fn id(&self) -> &'static str {
        "battlenet"
    }

    fn display_name(&self) -> &'static str {
        "Battle.net"
    }

    fn detect(&self) -> bool {
        let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
        hklm.open_subkey("SOFTWARE\\WOW6432Node\\Blizzard Entertainment\\Battle.net\\Capabilities")
            .is_ok()
    }

    fn list_games(&self) -> Result<Vec<Game>, String> {
        let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
        let mut games = Vec::new();
        // Read the newest log once up front rather than once per installed
        // game — see latest_log_content's doc comment.
        let log_content = Self::latest_log_content();

        for uninstall_path in UNINSTALL_KEYS {
            let Ok(uninstall_key) = hklm.open_subkey(uninstall_path) else {
                continue;
            };
            for subkey_name in uninstall_key.enum_keys().flatten() {
                let Ok(entry) = uninstall_key.open_subkey(&subkey_name) else {
                    continue;
                };
                let uninstall_string: String = entry.get_value("UninstallString").unwrap_or_default();
                if !uninstall_string.contains("Battle.net") || !uninstall_string.contains("--uid=") {
                    continue;
                }
                let Some(uid) = uninstall_string.split("--uid=").nth(1) else {
                    continue;
                };
                let uid = uid.split_whitespace().next().unwrap_or(uid).trim();
                // uid=battle.net is the Battle.net client's own uninstaller
                // entry (verified live: "Blizzard Uninstaller.exe" --uid=
                // battle.net --displayname="Battle.net"), not a game.
                if uid == "battle.net" || uid.is_empty() {
                    continue;
                }
                let name = KNOWN_PRODUCTS
                    .iter()
                    .find(|(id, _)| *id == uid)
                    .map(|(_, n)| n.to_string())
                    .unwrap_or_else(|| uid.to_string());
                let version: Option<String> = entry.get_value("DisplayVersion").ok();

                games.push(Game {
                    launcher: "battlenet",
                    id: uid.to_string(),
                    name,
                    installed_build: version,
                    status: if log_content.as_deref().is_some_and(|c| Self::is_updating(c, uid)) {
                        UpdateStatus::Updating
                    } else {
                        UpdateStatus::Unknown
                    },
                    size_bytes: None,
                    last_updated: None,
                });
            }
        }
        Ok(games)
    }

    // Verified live: the Capabilities registry key's own ApplicationIcon
    // value on this machine points at "...\Battle.net\Battle.net.exe".
    fn process_names(&self) -> &'static [&'static str] {
        &["Battle.net.exe"]
    }

    fn icon_source(&self) -> Option<std::path::PathBuf> {
        let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
        let icon_value: String = hklm
            .open_subkey("SOFTWARE\\WOW6432Node\\Blizzard Entertainment\\Battle.net\\Capabilities")
            .ok()?
            .get_value("ApplicationIcon")
            .ok()?;
        // Real value looks like "X:\Battle.net\Battle.net.exe,0" — the
        // icon-index suffix isn't a valid path component.
        let path = icon_value.rsplit_once(',').map(|(p, _)| p).unwrap_or(&icon_value);
        Some(std::path::PathBuf::from(path))
    }

    fn trigger_update(&self, _game_id: &str) -> Result<(), String> {
        open::that("battlenet://").map_err(|e| e.to_string())
    }
}
