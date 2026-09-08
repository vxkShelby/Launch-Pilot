pub mod epic;
pub mod gog;
pub mod steam;

use serde::Serialize;

#[derive(Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UpdateStatus {
    UpToDate,
    UpdateAvailable,
    Unknown,
}

#[derive(Serialize, Clone)]
pub struct Game {
    pub launcher: &'static str,
    pub id: String,
    pub name: String,
    pub installed_build: Option<String>,
    pub status: UpdateStatus,
    pub size_bytes: Option<u64>,
    pub last_updated: Option<u64>,
}

pub trait LauncherProvider: Send + Sync {
    fn id(&self) -> &'static str;
    #[allow(dead_code)] // wired into the dashboard in Phase 4
    fn display_name(&self) -> &'static str;
    /// Whether this launcher is installed on the machine.
    fn detect(&self) -> bool;
    fn list_games(&self) -> Result<Vec<Game>, String>;
    /// Best-effort update trigger. Returns Err if no automated path exists
    /// for this launcher — callers should fall back to a deep link.
    fn trigger_update(&self, game_id: &str) -> Result<(), String>;
}

pub fn all_providers() -> Vec<Box<dyn LauncherProvider>> {
    vec![
        Box::new(steam::SteamProvider),
        Box::new(epic::EpicProvider),
        Box::new(gog::GogProvider),
    ]
}
