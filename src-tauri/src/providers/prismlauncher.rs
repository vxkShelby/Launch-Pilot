// PrismLauncher provider (open-source Minecraft launcher).
//
// Detection: %APPDATA%\PrismLauncher\prismlauncher.cfg exists — written on
// first run. Verified against a real local install.
// Enumeration: the config's [General] InstanceDir key (relative to
// %APPDATA%\PrismLauncher unless it's an absolute path) points at a folder
// of one subdirectory per instance, each with:
//   - instance.cfg: flat INI, `name=` and `lastLaunchTime=` (ms epoch)
//   - mmc-pack.json: real JSON, a "components" array; the entry with
//     uid "net.minecraft" carries the actual Minecraft version in
//     cachedVersion.
// Both verified directly against a real instance on this machine (schema is
// also just PrismLauncher's own documented format, since it's open source —
// nothing here is reverse-engineered guesswork).
//
// Update status: instances are versions the user deliberately pinned, so
// "outdated" isn't quite right — but whether a newer Minecraft release
// exists at all is real, public, verifiable data: Mojang's own
// version_manifest_v2.json (launchermeta.mojang.com/mc/game/
// version_manifest_v2.json) is documented, auth-free, and was fetched live
// during development (returned real current `latest.release`, e.g. "26.2").
// Instances pinned to net.minecraft's cachedVersion (already read below)
// that don't match that latest release are reported as UpdateAvailable —
// "a newer release exists," not "you must update," since pinning an old
// version is often deliberate (mod compatibility). Best-effort: any
// network failure falls back to honest Unknown, same as GOG.
// No verified per-instance deep link exists, so trigger_update just opens
// the launcher itself.
use super::{Game, LauncherProvider, UpdateStatus};
use crate::ini;
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Deserialize)]
struct VersionManifest {
    latest: LatestVersions,
}

#[derive(Deserialize)]
struct LatestVersions {
    release: String,
}

#[derive(Deserialize)]
struct MmcPack {
    components: Vec<MmcComponent>,
}

#[derive(Deserialize)]
struct MmcComponent {
    uid: String,
    #[serde(rename = "cachedVersion")]
    cached_version: Option<String>,
}

pub struct PrismLauncherProvider;

impl PrismLauncherProvider {
    fn config_dir(&self) -> Option<PathBuf> {
        let appdata = std::env::var("APPDATA").ok()?;
        Some(PathBuf::from(appdata).join("PrismLauncher"))
    }

    fn config_file(&self) -> Option<PathBuf> {
        Some(self.config_dir()?.join("prismlauncher.cfg"))
    }

    fn instances_dir(&self) -> Option<PathBuf> {
        let config_dir = self.config_dir()?;
        let content = std::fs::read_to_string(config_dir.join("prismlauncher.cfg")).ok()?;
        let cfg = ini::parse(&content);
        let instance_dir = cfg.get("InstanceDir").cloned().unwrap_or_else(|| "instances".to_string());
        let path = PathBuf::from(&instance_dir);
        Some(if path.is_absolute() { path } else { config_dir.join(path) })
    }

    fn minecraft_version(instance_path: &std::path::Path) -> Option<String> {
        let content = std::fs::read_to_string(instance_path.join("mmc-pack.json")).ok()?;
        let pack: MmcPack = serde_json::from_str(&content).ok()?;
        pack.components
            .into_iter()
            .find(|c| c.uid == "net.minecraft")
            .and_then(|c| c.cached_version)
    }

    fn exe_path(&self) -> Option<PathBuf> {
        let local_appdata = std::env::var("LOCALAPPDATA").ok()?;
        Some(PathBuf::from(local_appdata).join("Programs\\PrismLauncher\\prismlauncher.exe"))
    }

    fn latest_minecraft_release() -> Option<String> {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(4))
            .build()
            .ok()?;
        let manifest: VersionManifest = client
            .get("https://launchermeta.mojang.com/mc/game/version_manifest_v2.json")
            .send()
            .ok()?
            .json()
            .ok()?;
        Some(manifest.latest.release)
    }
}

impl LauncherProvider for PrismLauncherProvider {
    fn id(&self) -> &'static str {
        "prismlauncher"
    }

    fn display_name(&self) -> &'static str {
        "PrismLauncher"
    }

    fn detect(&self) -> bool {
        self.config_file().is_some_and(|p| p.exists())
    }

    fn list_games(&self) -> Result<Vec<Game>, String> {
        let dir = self.instances_dir().ok_or("PrismLauncher not configured")?;
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return Ok(Vec::new());
        };

        let latest_release = Self::latest_minecraft_release();

        let mut games = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            let cfg_path = path.join("instance.cfg");
            let Ok(content) = std::fs::read_to_string(&cfg_path) else {
                continue;
            };
            let cfg = ini::parse(&content);
            let dir_name = path.file_name().and_then(|f| f.to_str()).unwrap_or("instance").to_string();
            let name = cfg.get("name").cloned().unwrap_or_else(|| dir_name.clone());
            let last_played_ms: Option<u64> = cfg.get("lastLaunchTime").and_then(|s| s.parse().ok());
            let installed_version = Self::minecraft_version(&path);

            let status = match (&installed_version, &latest_release) {
                (Some(installed), Some(latest)) if installed != latest => UpdateStatus::UpdateAvailable,
                (Some(_), Some(_)) => UpdateStatus::UpToDate,
                _ => UpdateStatus::Unknown,
            };

            games.push(Game {
                launcher: "prismlauncher",
                id: dir_name,
                name,
                installed_build: installed_version,
                status,
                size_bytes: None,
                last_updated: last_played_ms.map(|ms| ms / 1000),
            });
        }
        Ok(games)
    }

    fn process_names(&self) -> &'static [&'static str] {
        &["prismlauncher.exe"]
    }

    fn trigger_update(&self, _game_id: &str) -> Result<(), String> {
        let exe = self.exe_path().ok_or("PrismLauncher not installed")?;
        std::process::Command::new(exe)
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}
