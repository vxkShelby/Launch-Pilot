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
    /// Ids of installed launchers whose own client process is running
    /// right now, from a single real `tasklist` snapshot — a launcher
    /// being installed doesn't mean it's open.
    pub running_launchers: Vec<&'static str>,
}

/// Real, single Windows process snapshot via the built-in `tasklist` tool
/// (no extra dependency, works on any Windows install) — one call covers
/// every provider instead of spawning a process per launcher.
///
/// Tried and reverted: gating this on "owns a visible top-level window"
/// (via EnumWindows/IsWindowVisible) to fix a real false positive — Ubisoft
/// Connect's `upc.exe` stays alive as a backgrounded/tray process after the
/// user closes the client, so process-presence alone reports it as running
/// when it isn't. But verified live with a raw EnumWindows dump that this
/// doesn't actually distinguish the cases: Steam, while genuinely open but
/// minimized to tray, owns only the exact same shape of window — hidden
/// (IsWindowVisible=false), with a real title ("Steam" / "Ubisoft
/// Connect"). Windows doesn't expose a reliable "is this a real closed
/// client vs. minimized-to-tray" signal short of parsing the notification
/// area itself, which is out of scope. Reverted to plain process presence:
/// correct for every launcher tested except Ubisoft Connect's tray-resident
/// helper, a disclosed known limitation rather than a fragile heuristic
/// that broke Steam to partially fix Ubisoft.
fn running_process_names() -> std::collections::HashSet<String> {
    let Ok(output) = std::process::Command::new("tasklist").arg("/FO").arg("CSV").arg("/NH").output() else {
        return std::collections::HashSet::new();
    };
    let text = String::from_utf8_lossy(&output.stdout);
    text.lines()
        .filter_map(|line| line.split(',').next())
        .map(|name| name.trim_matches('"').to_ascii_lowercase())
        .collect()
}

/// Single pass over all providers — detect() and list_games() run exactly
/// once each, instead of once per command as with two separate commands.
#[tauri::command]
pub fn dashboard_data() -> DashboardData {
    let mut games = Vec::new();
    let mut connected_only = Vec::new();
    let mut running_launchers = Vec::new();
    let running = running_process_names();

    for provider in all_providers() {
        if !provider.detect() {
            continue;
        }
        if provider
            .process_names()
            .iter()
            .any(|name| running.contains(&name.to_ascii_lowercase()))
        {
            running_launchers.push(provider.id());
        }
        match provider.list_games() {
            Ok(found) if !found.is_empty() => games.extend(found),
            _ => connected_only.push(LauncherInfo {
                id: provider.id(),
                name: provider.display_name(),
            }),
        }
    }

    DashboardData { games, connected_only, running_launchers }
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
