// Wargaming.net Game Center provider (World of Tanks, World of Warships,
// World of Warplanes).
//
// Detection + game discovery: %ProgramData%\Wargaming.net\GameCenter\
// preferences.xml — real, live-read XML (verified against a real World of
// Tanks install on this machine) whose <games_manager><games><game>
// <working_dir> entries list every game GameCenter manages, independent of
// an earlier, wrong path (apps\<id>\ under the same ProgramData root, which
// turned out to be empty and unrelated).
//
// Update status: each game's own <working_dir>\game_info.xml has a real,
// already-present "installed vs. available" comparison per component —
// `<version name="client" installed="2.4.0.24018" available="2.4.0.24018"/>`
// — verified live. This is a genuine local update-pending signal, better
// than most other providers here: if any component's installed/available
// differ, an update is available; otherwise up to date.
use super::{Game, LauncherProvider, UpdateStatus};
use quick_xml::events::Event;
use quick_xml::Reader;
use std::path::PathBuf;

pub struct WargamingProvider;

impl WargamingProvider {
    fn preferences_path(&self) -> Option<PathBuf> {
        let program_data = std::env::var("ProgramData").ok()?;
        Some(PathBuf::from(program_data).join("Wargaming.net\\GameCenter\\preferences.xml"))
    }

    fn working_dirs(&self) -> Vec<String> {
        let Some(path) = self.preferences_path() else {
            return Vec::new();
        };
        let Ok(content) = std::fs::read_to_string(&path) else {
            return Vec::new();
        };

        let mut reader = Reader::from_str(&content);
        let mut dirs = Vec::new();
        let mut in_games = false;
        let mut in_working_dir = false;
        let mut buf = Vec::new();

        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(e)) if e.local_name().as_ref() == "games" => in_games = true,
                Ok(Event::End(e)) if e.local_name().as_ref() == "games" => in_games = false,
                Ok(Event::Start(e)) if in_games && e.local_name().as_ref() == "working_dir" => {
                    in_working_dir = true;
                }
                Ok(Event::Text(t)) if in_working_dir => dirs.push(t.as_ref().to_string()),
                Ok(Event::End(e)) if e.local_name().as_ref() == "working_dir" => in_working_dir = false,
                Ok(Event::Eof) => break,
                Err(_) => break,
                _ => {}
            }
            buf.clear();
        }
        dirs
    }

    fn read_game_info(working_dir: &str) -> Option<Game> {
        let content = std::fs::read_to_string(PathBuf::from(working_dir).join("game_info.xml")).ok()?;
        let mut reader = Reader::from_str(&content);
        let mut buf = Vec::new();

        let mut id = None;
        let mut version_name = None;
        let mut has_pending_update = false;
        let mut current_tag: Option<String> = None;

        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(e)) => current_tag = Some(e.local_name().as_ref().to_string()),
                Ok(Event::Text(t)) => {
                    if let Some(tag) = &current_tag {
                        match tag.as_str() {
                            "id" => id = Some(t.as_ref().to_string()),
                            "version_name" => version_name = Some(t.as_ref().to_string()),
                            _ => {}
                        }
                    }
                }
                Ok(Event::Empty(e)) if e.local_name().as_ref() == "version" => {
                    let mut installed = None;
                    let mut available = None;
                    for attr in e.attributes().flatten() {
                        match attr.key.as_ref() {
                            "installed" => installed = Some(attr.value.into_owned()),
                            "available" => available = Some(attr.value.into_owned()),
                            _ => {}
                        }
                    }
                    if let (Some(i), Some(a)) = (installed, available) {
                        if i != a {
                            has_pending_update = true;
                        }
                    }
                }
                Ok(Event::End(_)) => current_tag = None,
                Ok(Event::Eof) => break,
                Err(_) => break,
                _ => {}
            }
            buf.clear();
        }

        let folder_name = PathBuf::from(working_dir)
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or("game")
            .replace('_', " ");

        Some(Game {
            launcher: "wargaming",
            id: id.unwrap_or_else(|| folder_name.clone()),
            name: folder_name,
            installed_build: version_name,
            status: if has_pending_update { UpdateStatus::UpdateAvailable } else { UpdateStatus::UpToDate },
            size_bytes: None,
            last_updated: None,
        })
    }
}

impl LauncherProvider for WargamingProvider {
    fn id(&self) -> &'static str {
        "wargaming"
    }

    fn display_name(&self) -> &'static str {
        "Wargaming Game Center"
    }

    fn detect(&self) -> bool {
        self.preferences_path().is_some_and(|p| p.exists())
    }

    fn list_games(&self) -> Result<Vec<Game>, String> {
        Ok(self.working_dirs().iter().filter_map(|dir| Self::read_game_info(dir)).collect())
    }

    fn trigger_update(&self, _game_id: &str) -> Result<(), String> {
        // Game Center's client (wgc.exe) has no registry footprint either
        // (verified: no HKLM uninstall/App Paths entry exists for it on
        // this machine) — its real location was only found by resolving
        // this machine's own Desktop shortcut, which pointed at
        // <drive>:\Wargaming.net\GameCenter\wgc.exe. Same disclosed
        // heuristic as the Age of the Ring provider: scan drive letters for
        // that path rather than hardcode this machine's drive letter.
        for drive in 'C'..='Z' {
            let candidate = PathBuf::from(format!("{drive}:\\Wargaming.net\\GameCenter\\wgc.exe"));
            if candidate.exists() {
                return std::process::Command::new(candidate).spawn().map(|_| ()).map_err(|e| e.to_string());
            }
        }
        Err("Wargaming Game Center executable not found".to_string())
    }

    fn process_names(&self) -> &'static [&'static str] {
        &["wgc.exe"]
    }
}
