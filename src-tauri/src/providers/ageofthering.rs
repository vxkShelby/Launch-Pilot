// "Age of the Ring" provider — a fan-made Battle for Middle-earth II mod
// launcher (AotR_Launcher.exe). Confirmed real and worth supporting: it
// ships a genuine changelog with a parseable current version, not just a
// single-community toy.
//
// Detection: this launcher has NO installer footprint at all — verified by
// checking the uninstall registry, Start Menu, and Desktop shortcuts on a
// real machine that has it installed: none exist. It's manually extracted
// by the user to a folder, typically named "AgeoftheRing" at a drive root
// (that's what's actually on this machine). So detection here is a
// disclosed heuristic, not a verified signal like the other providers:
// scan common drive letters for `<drive>:\AgeoftheRing\AotR_Launcher.exe`.
// This will miss installs extracted somewhere else — an honest limitation,
// not a guess dressed up as certainty.
//
// Version: `aotr\ChangelistFormatted.txt`'s first line is a real, current
// changelog header — `== Version 9.3.3 - August 23rd 2026 ==` — verified
// directly against a real install. No update-availability signal exists
// locally (would need a network check against the mod's own site, which is
// out of scope), so status is honest Unknown.
//
// This isn't a single-game launcher: the same root folder holds the two
// base EA games it patches (bfme2 = Battle for Middle-earth II, rotwk =
// Rise of the Witch-king — both verified real on this machine, complete
// with their own eauninstall.exe) alongside the mod overlay itself (aotr).
// list_games() reports one row per component folder actually present,
// instead of a single hardcoded "Age of the Ring" row.
use super::{Game, LauncherProvider, UpdateStatus};
use std::path::PathBuf;

const RELATIVE_EXE: &str = "AgeoftheRing\\AotR_Launcher.exe";

const COMPONENTS: &[(&str, &str)] = &[
    ("aotr", "Age of the Ring (mod)"),
    ("bfme2", "Battle for Middle-earth II"),
    ("rotwk", "Rise of the Witch-king"),
];

pub struct AgeOfTheRingProvider;

impl AgeOfTheRingProvider {
    fn find_install(&self) -> Option<PathBuf> {
        for drive in 'C'..='Z' {
            let path = PathBuf::from(format!("{drive}:\\{RELATIVE_EXE}"));
            if path.exists() {
                return Some(path);
            }
        }
        None
    }

    fn version(exe_path: &std::path::Path) -> Option<String> {
        let root = exe_path.parent()?;
        let content = std::fs::read_to_string(root.join("aotr").join("ChangelistFormatted.txt")).ok()?;
        let first_line = content.lines().next()?;
        // "== Version 9.3.3 - August 23rd 2026 ==" -> "9.3.3"
        first_line
            .split("Version")
            .nth(1)?
            .split('-')
            .next()
            .map(|s| s.trim().to_string())
    }
}

impl LauncherProvider for AgeOfTheRingProvider {
    fn id(&self) -> &'static str {
        "ageofthering"
    }

    fn display_name(&self) -> &'static str {
        "Age of the Ring"
    }

    fn detect(&self) -> bool {
        self.find_install().is_some()
    }

    fn list_games(&self) -> Result<Vec<Game>, String> {
        let Some(exe_path) = self.find_install() else {
            return Ok(Vec::new());
        };
        let Some(root) = exe_path.parent() else {
            return Ok(Vec::new());
        };
        let mut games = Vec::new();
        for (id, name) in COMPONENTS {
            if !root.join(id).is_dir() {
                continue;
            }
            games.push(Game {
                launcher: "ageofthering",
                id: id.to_string(),
                name: name.to_string(),
                installed_build: if *id == "aotr" { Self::version(&exe_path) } else { None },
                status: UpdateStatus::Unknown,
                size_bytes: None,
                last_updated: None,
            });
        }
        Ok(games)
    }

    fn trigger_update(&self, _game_id: &str) -> Result<(), String> {
        let exe = self.find_install().ok_or("Age of the Ring not found")?;
        std::process::Command::new(exe)
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}
