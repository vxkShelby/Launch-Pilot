use crate::providers::{all_providers, Game};
use serde::Serialize;

#[tauri::command]
pub fn list_games() -> Vec<Game> {
    let mut games = Vec::new();
    for provider in all_providers() {
        if provider.detect() {
            if let Ok(mut found) = provider.list_games() {
                games.append(&mut found);
            }
        }
    }
    games
}

#[derive(Serialize)]
pub struct LauncherInfo {
    id: &'static str,
    name: &'static str,
}

/// Detected launchers that contributed no games to list_games — either
/// nothing is installed through them, or (EA/Ubisoft/Battle.net) we only
/// support detect+deep-link for that launcher. Lets the dashboard show
/// "connected, open it yourself" for launchers with no enumerable games.
#[tauri::command]
pub fn list_launchers_without_games() -> Vec<LauncherInfo> {
    all_providers()
        .into_iter()
        .filter(|p| p.detect())
        .filter(|p| p.list_games().map(|g| g.is_empty()).unwrap_or(true))
        .map(|p| LauncherInfo {
            id: p.id(),
            name: p.display_name(),
        })
        .collect()
}

#[tauri::command]
pub fn trigger_update(launcher: String, game_id: String) -> Result<(), String> {
    let provider = all_providers()
        .into_iter()
        .find(|p| p.id() == launcher)
        .ok_or_else(|| format!("unknown launcher: {launcher}"))?;
    provider.trigger_update(&game_id)
}
