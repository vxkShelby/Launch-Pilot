pub mod battlenet;
pub mod ea;
pub mod epic;
pub mod gog;
pub mod prismlauncher;
pub mod riot;
pub mod steam;
pub mod ubisoft;

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
    fn display_name(&self) -> &'static str;
    /// Whether this launcher is installed on the machine.
    fn detect(&self) -> bool;
    fn list_games(&self) -> Result<Vec<Game>, String>;
    /// Best-effort update trigger. Empty game_id targets the launcher itself
    /// rather than a specific game, for launchers without per-game deep links.
    fn trigger_update(&self, game_id: &str) -> Result<(), String>;
}

pub fn all_providers() -> Vec<Box<dyn LauncherProvider>> {
    vec![
        Box::new(steam::SteamProvider),
        Box::new(epic::EpicProvider),
        Box::new(gog::GogProvider),
        Box::new(ea::EaProvider),
        Box::new(ubisoft::UbisoftProvider),
        Box::new(battlenet::BattleNetProvider),
        Box::new(riot::RiotProvider),
        Box::new(prismlauncher::PrismLauncherProvider),
    ]
}
