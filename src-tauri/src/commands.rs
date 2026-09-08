use crate::providers::{all_providers, Game};
use serde::Serialize;

#[derive(Serialize)]
pub struct LauncherInfo {
    id: &'static str,
    name: &'static str,
}

#[derive(Serialize)]
pub struct DashboardData {
    pub games: Vec<Game>,
    /// Detected launchers that contributed no games — either nothing is
    /// installed through them, or (EA/Ubisoft/Battle.net) we only support
    /// detect+deep-link for that launcher. Lets the dashboard show
    /// "connected, open it yourself" instead of silently omitting them.
    pub connected_only: Vec<LauncherInfo>,
}

/// Single pass over all providers — detect() and list_games() run exactly
/// once each, instead of once per command as with two separate commands.
#[tauri::command]
pub fn dashboard_data() -> DashboardData {
    let mut games = Vec::new();
    let mut connected_only = Vec::new();

    for provider in all_providers() {
        if !provider.detect() {
            continue;
        }
        match provider.list_games() {
            Ok(found) if !found.is_empty() => games.extend(found),
            _ => connected_only.push(LauncherInfo {
                id: provider.id(),
                name: provider.display_name(),
            }),
        }
    }

    DashboardData { games, connected_only }
}

/// Rejects anything that isn't a plausible id before it reaches a shell-out
/// or URI (steam://, com.epicgames.launcher://, goggalaxy://, RiotClient
/// process args) — defense in depth against a compromised/XSS'd frontend
/// calling trigger_update directly with an attacker-chosen game_id.
fn is_safe_id(id: &str) -> bool {
    id.is_empty() || id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

#[tauri::command]
pub fn trigger_update(launcher: String, game_id: String) -> Result<(), String> {
    if !is_safe_id(&game_id) {
        return Err("invalid game id".to_string());
    }
    let provider = all_providers()
        .into_iter()
        .find(|p| p.id() == launcher)
        .ok_or_else(|| format!("unknown launcher: {launcher}"))?;
    provider.trigger_update(&game_id)
}
