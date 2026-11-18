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
// like `D 2026-10-26 20:25:48.415683 [InstallInfo] {Main} Operation status
// changed: opType=Update oldStatus=Off newStatus=On agentUid=s2` (copied
// from this machine's real logs: the client's own self-update and a
// StarCraft II update). opType=Update + newStatus=On for a matching
// agentUid means actively updating right now. The same logs also have
// opType=Execute lines (e.g. `Execute ... newStatus=On agentUid=s2` for
// five minutes, then Off); these are NOT updates and are ignored. That
// Execute means "game running" is inferred, not verified.
//
// History: this matched the tag `[InstallManager]`, which the real
// "Operation status changed" lines never carry (they carry `[InstallInfo]`),
// so an update could never be detected from the current client's logs. The
// match is now on the message text and opType, not the tag, and the log
// picker only considers `battle.net-*.log` (libcef-*.log files sit in the
// same folder and could have been picked as "newest").
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

/// `battle.net-<timestamp>.log` only; the folder also holds `libcef-*.log`.
fn is_client_log(file_name: &str) -> bool {
    file_name.starts_with("battle.net-") && file_name.ends_with(".log")
}

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
            .filter(|e| is_client_log(&e.file_name().to_string_lossy()))
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
            // Whole-token match: a substring test would let `agentUid=wow`
            // also match `agentUid=wow_classic`. Only opType=Update counts:
            // Execute lines are not updates.
            .find(|line| {
                line.contains("Operation status changed:")
                    && line.split_whitespace().any(|t| t == marker)
                    && line.split_whitespace().any(|t| t == "opType=Update")
            })
            .is_some_and(|line| line.split_whitespace().any(|t| t == "newStatus=On"))
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
        // Staleness guard: the log only records status *changes*, so if
        // Battle.net exits or crashes mid-update the last line for the game
        // stays `newStatus=On` forever. An update can only be running while
        // the client process is alive, so without it the log is ignored.
        let client_running = super::running_process_names().contains("battle.net.exe");

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
                    status: if client_running && log_content.as_deref().is_some_and(|c| Self::is_updating(c, uid)) {
                        UpdateStatus::Updating
                    } else {
                        UpdateStatus::Unknown
                    },
                    ..Game::default()
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

#[cfg(test)]
mod tests {
    use super::*;

    // Line shape copied from this machine's real battle.net-*.log.
    fn line_op(op: &str, status: &str, uid: &str) -> String {
        format!("D 2026-10-26 20:25:48.415683 [InstallInfo] {{Main}} Operation status changed: opType={op} oldStatus=Off newStatus={status} agentUid={uid}")
    }
    fn line(status: &str, uid: &str) -> String {
        line_op("Update", status, uid)
    }

    #[test]
    fn real_log_line_shape_is_recognised() {
        let real = "D 2026-10-26 20:25:48.415683 [InstallInfo] {Main} Operation status changed: opType=Update oldStatus=Off newStatus=On agentUid=s2";
        assert!(BattleNetProvider::is_updating(real, "s2"));
    }

    #[test]
    fn execute_operations_are_not_updates() {
        // Real log: `opType=Execute ... newStatus=On agentUid=s2` lasted 5 minutes.
        assert!(!BattleNetProvider::is_updating(&line_op("Execute", "On", "s2"), "s2"));
        // An Execute line after an Update-On does not end the update.
        let log = format!("{}\n{}\n", line("On", "s2"), line_op("Execute", "Off", "s2"));
        assert!(BattleNetProvider::is_updating(&log, "s2"));
    }

    #[test]
    fn pending_is_not_updating() {
        assert!(!BattleNetProvider::is_updating(&line("Pending", "s2"), "s2"));
    }

    #[test]
    fn only_client_logs_are_candidates() {
        assert!(is_client_log("battle.net-20261026T203600.443848.log"));
        assert!(!is_client_log("libcef-20261026T203604.521014.log"));
        assert!(!is_client_log("battle.net-20261026T203600.443848.log.old"));
    }

    #[test]
    fn latest_status_line_for_the_uid_decides() {
        let log = format!("{}\n{}\n", line("On", "wow"), line("Off", "wow"));
        assert!(!BattleNetProvider::is_updating(&log, "wow"));
        let log = format!("{}\n{}\n", line("Off", "wow"), line("On", "wow"));
        assert!(BattleNetProvider::is_updating(&log, "wow"));
    }

    #[test]
    fn uid_that_is_a_prefix_of_another_uid_does_not_match() {
        assert!(!BattleNetProvider::is_updating(&line("On", "wow_classic"), "wow"));
        assert!(BattleNetProvider::is_updating(&line("On", "wow_classic"), "wow_classic"));
    }

    #[test]
    fn other_games_lines_are_ignored() {
        assert!(!BattleNetProvider::is_updating(&line("On", "d3"), "wow"));
    }
}
