use crate::providers::{all_providers, Game};

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

#[tauri::command]
pub fn trigger_update(launcher: String, game_id: String) -> Result<(), String> {
    let provider = all_providers()
        .into_iter()
        .find(|p| p.id() == launcher)
        .ok_or_else(|| format!("unknown launcher: {launcher}"))?;
    provider.trigger_update(&game_id)
}
