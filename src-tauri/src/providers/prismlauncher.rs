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
// Update status, reworked per user feedback: comparing an instance's
// pinned Minecraft version against Mojang's latest release (the previous
// approach) was wrong — instances are DELIBERATELY pinned old for mod
// compatibility, so that comparison flagged nearly every real instance as
// "needs update" when nothing was actually wrong. Replaced with two real,
// separate checks instead:
//
// 1. The launcher APP itself: PrismLauncher is open source
//    (github.com/PrismLauncher/PrismLauncher), so its own version is real,
//    checkable data — verified live: the installed exe's own FileVersion
//    (Win32 VersionInfo, read via a hidden PowerShell `Get-Item
//    .VersionInfo.FileVersion` call, e.g. "11.1.0.0") compared against
//    GitHub's real releases API (api.github.com/repos/PrismLauncher/
//    PrismLauncher/releases/latest, tag_name "11.1.0" — fetched live, no
//    auth). Shown as a synthetic "PrismLauncher (app)" row alongside the
//    real instances. This is the ONE launcher in this app where a generic
//    "is the launcher itself outdated" check is honestly possible — every
//    other launcher here is closed-source with no public version feed
//    (investigated and confirmed NOT VIABLE for Steam/EA/Ubisoft/
//    Battle.net/Riot/GOG Galaxy/Wargaming/Age of the Ring), so this stays
//    PrismLauncher-only rather than a fabricated "coming soon" for others.
//
// 2. Per-instance mod updates: real IF the instance was imported as a
//    Modrinth modpack — Modrinth's own documented pack format
//    (modrinth.index.json) lists each mod file's real download URL, which
//    embeds a real project id + version id
//    (cdn.modrinth.com/data/<project>/versions/<version>/<file>). Modrinth's
//    public API (api.modrinth.com/v2/project/<id>/version, verified live
//    auth-free, e.g. fetching Sodium's real version list) can then say
//    whether a newer version exists per mod. CORROBORATED, not verified
//    end-to-end here: no instance on this machine actually has a
//    modrinth.index.json (the one real instance here was assembled by
//    hand, dropping jars into mods\ with zero version metadata) — the
//    parsing logic follows Modrinth's documented format, but the index
//    file's exact placement in a Prism-imported pack wasn't directly
//    observed. Honest Unknown whenever the file isn't found, which is
//    every hand-assembled instance (the common case) — no fabricated
//    per-mod status from jar filenames.
use super::{Game, LauncherProvider, UpdateStatus};
use crate::ini;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::time::Duration;

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

#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
}

#[derive(Deserialize)]
struct ModrinthIndex {
    files: Vec<ModrinthFile>,
}

#[derive(Deserialize)]
struct ModrinthFile {
    downloads: Vec<String>,
}

#[derive(Deserialize)]
struct ModrinthVersion {
    id: String,
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

    /// Real Win32 file-version read via a hidden PowerShell call — avoids
    /// hand-rolling VS_FIXEDFILEINFO parsing over raw FFI for one string.
    fn installed_version(&self) -> Option<String> {
        let exe = self.exe_path()?;
        let mut command = std::process::Command::new("powershell");
        command
            .arg("-NoProfile")
            .arg("-Command")
            .arg(format!("(Get-Item '{}').VersionInfo.FileVersion", exe.display().to_string().replace('\'', "''")));
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x08000000;
            command.creation_flags(CREATE_NO_WINDOW);
        }
        let output = command.output().ok()?;
        let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if text.is_empty() {
            None
        } else {
            Some(text)
        }
    }

    fn latest_release_tag() -> Option<String> {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(4))
            .user_agent("LaunchPilot")
            .build()
            .ok()?;
        let release: GithubRelease = client
            .get("https://api.github.com/repos/PrismLauncher/PrismLauncher/releases/latest")
            .send()
            .ok()?
            .json()
            .ok()?;
        Some(release.tag_name)
    }

    /// Compares only the first three numeric components (major.minor.patch)
    /// since the exe's FileVersion carries a trailing build/revision field
    /// ("11.1.0.0") that the GitHub tag ("11.1.0") doesn't — verified live
    /// these represent the same real release.
    fn version_triplet(s: &str) -> Vec<u32> {
        s.trim_start_matches('v').split('.').take(3).filter_map(|p| p.parse().ok()).collect()
    }

    fn app_update_status(&self) -> (Option<String>, UpdateStatus) {
        let installed = self.installed_version();
        let latest = Self::latest_release_tag();
        let status = match (&installed, &latest) {
            (Some(i), Some(l)) if Self::version_triplet(i) != Self::version_triplet(l) => UpdateStatus::UpdateAvailable,
            (Some(_), Some(_)) => UpdateStatus::UpToDate,
            _ => UpdateStatus::Unknown,
        };
        (installed, status)
    }

    /// Modrinth's documented pack-file download URL shape:
    /// cdn.modrinth.com/data/<project_id>/versions/<version_id>/<filename>
    ///
    /// Security-review finding, fixed: this file's own `downloads` URL comes
    /// from a `modrinth.index.json` the user imported — third-party content,
    /// not something LaunchPilot produced — and the extracted ids used to be
    /// spliced verbatim into a new request URL with no format check. A
    /// crafted index file could put `/`, `..`, or `?`/`#` into what's
    /// supposed to be a plain project/version id, redirecting the outgoing
    /// request's path/query within api.modrinth.com (the host itself is
    /// hardcoded, so this was never cross-host SSRF). Real Modrinth ids are
    /// short alphanumeric/`-`/`_` slugs or base62 ids, so reject anything
    /// else here rather than trusting the split result.
    fn is_valid_modrinth_id(s: &str) -> bool {
        !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
    }

    fn parse_modrinth_url(url: &str) -> Option<(String, String)> {
        let after = url.split("cdn.modrinth.com/data/").nth(1)?;
        let mut parts = after.split('/');
        let project_id = parts.next()?.to_string();
        if parts.next()? != "versions" {
            return None;
        }
        let version_id = parts.next()?.to_string();
        if !Self::is_valid_modrinth_id(&project_id) || !Self::is_valid_modrinth_id(&version_id) {
            return None;
        }
        Some((project_id, version_id))
    }

    fn find_modrinth_index(instance_path: &Path) -> Option<PathBuf> {
        for candidate in ["modrinth.index.json", "minecraft/modrinth.index.json", ".minecraft/modrinth.index.json"] {
            let p = instance_path.join(candidate);
            if p.exists() {
                return Some(p);
            }
        }
        None
    }

    /// Checks one mod's latest Modrinth version against the version it was
    /// pinned to in the pack index. None means it couldn't be checked
    /// (network failure, bad response) — same as `continue` in the old
    /// sequential loop.
    fn check_one_mod(client: &reqwest::blocking::Client, project_id: &str, version_id: &str) -> Option<bool> {
        let resp = client.get(format!("https://api.modrinth.com/v2/project/{project_id}/version")).send().ok()?;
        let versions = resp.json::<Vec<ModrinthVersion>>().ok()?;
        Some(versions.first().is_some_and(|latest| latest.id != version_id))
    }

    /// Same three-way logic the old sequential loop used — UpdateAvailable
    /// if any mod needs one, else UpToDate if at least one was successfully
    /// checked, else Unknown — but fans the per-mod network calls out
    /// concurrently (same `thread::scope` + `scope.spawn` + `.join()`
    /// pattern gog.rs already uses for its own one-call-per-item problem)
    /// instead of making them one at a time.
    fn mod_update_status(instance_path: &Path) -> UpdateStatus {
        let Some(index_path) = Self::find_modrinth_index(instance_path) else {
            return UpdateStatus::Unknown;
        };
        let Ok(content) = std::fs::read_to_string(&index_path) else {
            return UpdateStatus::Unknown;
        };
        let Ok(index) = serde_json::from_str::<ModrinthIndex>(&content) else {
            return UpdateStatus::Unknown;
        };
        let Ok(client) = reqwest::blocking::Client::builder().timeout(Duration::from_secs(4)).user_agent("LaunchPilot").build() else {
            return UpdateStatus::Unknown;
        };

        let checks: Vec<(String, String)> = index
            .files
            .iter()
            .filter_map(|file| Self::parse_modrinth_url(file.downloads.first()?))
            .collect();

        let results: Vec<Option<bool>> = std::thread::scope(|scope| {
            let handles: Vec<_> = checks
                .iter()
                .map(|(project_id, version_id)| scope.spawn(|| Self::check_one_mod(&client, project_id, version_id)))
                .collect();
            handles.into_iter().map(|h| h.join().unwrap_or(None)).collect()
        });

        let checked_any = results.iter().any(|r| r.is_some());
        if results.iter().any(|r| *r == Some(true)) {
            UpdateStatus::UpdateAvailable
        } else if checked_any {
            UpdateStatus::UpToDate
        } else {
            UpdateStatus::Unknown
        }
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

        let mut games = Vec::new();

        let (app_version, app_status) = self.app_update_status();
        games.push(Game {
            launcher: "prismlauncher",
            id: "__self__".to_string(),
            name: "PrismLauncher (app)".to_string(),
            installed_build: app_version,
            status: app_status,
            size_bytes: None,
            last_updated: None,
        });

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
            let status = Self::mod_update_status(&path);

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

    fn icon_source(&self) -> Option<PathBuf> {
        self.exe_path()
    }

    fn trigger_update(&self, _game_id: &str) -> Result<(), String> {
        let exe = self.exe_path().ok_or("PrismLauncher not installed")?;
        std::process::Command::new(exe)
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_triplet_ignores_trailing_build_revision() {
        // The whole reason this function exists: the exe's own FileVersion
        // carries a 4th component the GitHub tag doesn't, but both mean the
        // same release.
        assert_eq!(
            PrismLauncherProvider::version_triplet("11.1.0.0"),
            PrismLauncherProvider::version_triplet("11.1.0")
        );
    }

    #[test]
    fn version_triplet_strips_leading_v() {
        assert_eq!(PrismLauncherProvider::version_triplet("v11.1.0"), vec![11, 1, 0]);
    }

    #[test]
    fn version_triplet_empty_string_does_not_panic() {
        assert_eq!(PrismLauncherProvider::version_triplet(""), Vec::<u32>::new());
    }

    #[test]
    fn version_triplet_two_components_does_not_panic() {
        assert_eq!(PrismLauncherProvider::version_triplet("11.1"), vec![11, 1]);
    }

    #[test]
    fn parse_modrinth_url_extracts_project_and_version_id() {
        let url = "https://cdn.modrinth.com/data/AANobbMI/versions/abc123/sodium.jar";
        assert_eq!(
            PrismLauncherProvider::parse_modrinth_url(url),
            Some(("AANobbMI".to_string(), "abc123".to_string()))
        );
    }

    #[test]
    fn parse_modrinth_url_missing_versions_segment_returns_none() {
        let url = "https://cdn.modrinth.com/data/AANobbMI/abc123/sodium.jar";
        assert_eq!(PrismLauncherProvider::parse_modrinth_url(url), None);
    }

    #[test]
    fn parse_modrinth_url_without_the_data_host_path_returns_none() {
        let url = "https://example.com/not-modrinth-at-all";
        assert_eq!(PrismLauncherProvider::parse_modrinth_url(url), None);
    }

    #[test]
    fn parse_modrinth_url_rejects_traversal_in_project_id() {
        // Security-fix regression: ".." in the id slot must not slip through.
        let url = "https://cdn.modrinth.com/data/../versions/abc123/sodium.jar";
        assert_eq!(PrismLauncherProvider::parse_modrinth_url(url), None);
    }

    #[test]
    fn parse_modrinth_url_rejects_query_injection_in_version_id() {
        let url = "https://cdn.modrinth.com/data/AANobbMI/versions/abc?evil=1/sodium.jar";
        assert_eq!(PrismLauncherProvider::parse_modrinth_url(url), None);
    }

    #[test]
    fn parse_modrinth_url_rejects_path_separator_smuggled_via_encoding_attempt() {
        // A slash can't literally land inside one path segment, but a
        // segment that's nothing but disallowed characters (e.g. an
        // accidental double-slash producing an empty id) must still fail
        // closed rather than be treated as valid.
        let url = "https://cdn.modrinth.com/data//versions/abc123/sodium.jar";
        assert_eq!(PrismLauncherProvider::parse_modrinth_url(url), None);
    }
}
