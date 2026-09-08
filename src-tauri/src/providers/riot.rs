// Riot Client provider — detect + light enumeration, deep-link only.
//
// Detection + enumeration: %ProgramData%\Riot Games\RiotClientInstalls.json,
// a flat "product_key.shard": "install dir" map written by the Riot Client
// itself for every product it manages (League, VALORANT, etc), regardless
// of drive or custom install path. Verified against real open-source code
// that reads it (github.com/MouseWerk/lost-league-manager, league-path.js) —
// not observed locally since Riot Client isn't installed on this machine,
// but the source and schema are corroborated by that independent reader.
// No update-status field exists in it — honest Unknown, same as Epic/GOG.
//
// No verified riotclient:// URI protocol exists publicly, so trigger_update
// launches RiotClientServices.exe directly instead of guessing a scheme.
use super::{Game, LauncherProvider, UpdateStatus};
use std::path::PathBuf;

pub struct RiotProvider;

impl RiotProvider {
    fn installs_json(&self) -> Option<PathBuf> {
        let program_data = std::env::var("ProgramData").ok()?;
        Some(PathBuf::from(program_data).join("Riot Games\\RiotClientInstalls.json"))
    }

    fn client_exe(&self) -> Option<PathBuf> {
        let program_data = std::env::var("ProgramData").ok()?;
        Some(PathBuf::from(program_data).join("Riot Games\\Riot Client\\RiotClientServices.exe"))
    }

    fn friendly_name(product_key: &str) -> String {
        let base = product_key.split('.').next().unwrap_or(product_key);
        match base {
            "league_of_legends" => "League of Legends".to_string(),
            "valorant" => "VALORANT".to_string(),
            "bacon" => "Legends of Runeterra".to_string(),
            other => other.replace('_', " "),
        }
    }
}

impl LauncherProvider for RiotProvider {
    fn id(&self) -> &'static str {
        "riot"
    }

    fn display_name(&self) -> &'static str {
        "Riot Client"
    }

    fn detect(&self) -> bool {
        self.installs_json().is_some_and(|p| p.exists())
    }

    fn list_games(&self) -> Result<Vec<Game>, String> {
        let path = self.installs_json().ok_or("Riot Client not installed")?;
        let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
        let json: serde_json::Value = serde_json::from_str(&content).map_err(|e| e.to_string())?;
        let Some(map) = json.as_object() else {
            return Ok(Vec::new());
        };

        Ok(map
            .keys()
            .map(|key| Game {
                launcher: "riot",
                id: key.clone(),
                name: Self::friendly_name(key),
                installed_build: None,
                status: UpdateStatus::Unknown,
                size_bytes: None,
                last_updated: None,
            })
            .collect())
    }

    fn process_names(&self) -> &'static [&'static str] {
        &["RiotClientServices.exe"]
    }

    fn trigger_update(&self, _game_id: &str) -> Result<(), String> {
        let exe = self.client_exe().ok_or("Riot Client not installed")?;
        std::process::Command::new(exe)
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}
