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
//
//    "Latest" is filtered to what the instance can actually run. Modrinth's
//    project-versions endpoint takes `loaders` and `game_versions` (JSON
//    array strings, e.g. `["fabric"]`) and `include_changelog`:
//    https://docs.modrinth.com/api/operations/getprojectversions/
//    The instance's loader and Minecraft version come from its own
//    mmc-pack.json components (see `InstanceTarget`). Verified live: Sodium
//    unfiltered -> newest is a NeoForge build for a newer Minecraft;
//    filtered to fabric + 1.20.1 -> `mc1.20.1-0.5.13-fabric`.
//    Observed on this machine: Prism keeps an imported pack's original index
//    at `<instance>/mrpack/modrinth.index.json`, but it is NOT read here
//    (only 44 of its 169 files were still in that instance's mods folder, so
//    it no longer describes what is installed).
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
    version: Option<String>,
}

/// What an instance runs: the Minecraft version and (if any) mod loader, as
/// Modrinth names them. Read from mmc-pack.json component uids; the uids are
/// the ones Prism's own metadata index lists
/// (meta.prismlauncher.org/v1/index.json). Only Fabric is present in a real
/// instance on the reference machine; Quilt/Forge/NeoForge uids are from that
/// index, not from a local instance (unverified locally).
#[derive(Debug, PartialEq, Eq)]
struct InstanceTarget {
    minecraft: String,
    loader: Option<&'static str>,
}

fn loader_for_uid(uid: &str) -> Option<&'static str> {
    match uid {
        "net.fabricmc.fabric-loader" => Some("fabric"),
        "org.quiltmc.quilt-loader" => Some("quilt"),
        "net.minecraftforge" => Some("forge"),
        "net.neoforged" => Some("neoforge"),
        _ => None,
    }
}

fn instance_target(pack_json: &str) -> Option<InstanceTarget> {
    let pack: MmcPack = serde_json::from_str(pack_json).ok()?;
    let minecraft = pack
        .components
        .iter()
        .find(|c| c.uid == "net.minecraft")
        .and_then(|c| c.cached_version.clone().or_else(|| c.version.clone()))?;
    let loader = pack.components.iter().find_map(|c| loader_for_uid(&c.uid));
    Some(InstanceTarget { minecraft, loader })
}

/// Query for `GET /v2/project/{id}/version`. Mods are filtered by loader and
/// Minecraft version; other pack files (resource packs, shaders, datapacks
/// carry Modrinth loaders like `minecraft`/`iris`) by Minecraft version only.
/// A mod in an instance with no recognised loader cannot be filtered
/// honestly, so it yields `None` (the check becomes Unknown).
fn version_query(target: &InstanceTarget, is_mod: bool) -> Option<Vec<(&'static str, String)>> {
    let json = |v: &str| serde_json::to_string(&[v]).ok();
    let mut q = vec![("game_versions", json(&target.minecraft)?), ("include_changelog", "false".to_string())];
    if is_mod {
        q.push(("loaders", json(target.loader?)?));
    }
    Some(q)
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
    #[serde(default)]
    path: String,
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

    fn instance_target(instance_path: &std::path::Path) -> Option<InstanceTarget> {
        instance_target(&std::fs::read_to_string(instance_path.join("mmc-pack.json")).ok()?)
    }

    fn exe_path(&self) -> Option<PathBuf> {
        let local_appdata = std::env::var("LOCALAPPDATA").ok()?;
        Some(PathBuf::from(local_appdata).join("Programs\\PrismLauncher\\prismlauncher.exe"))
    }

    /// Real Win32 file-version read via a hidden PowerShell call — avoids
    /// hand-rolling VS_FIXEDFILEINFO parsing over raw FFI for one string.
    fn installed_version(&self) -> Option<String> {
        let exe = self.exe_path()?;
        let mut command = std::process::Command::new(super::powershell_exe());
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
    fn check_one_mod(
        client: &reqwest::blocking::Client,
        target: &InstanceTarget,
        project_id: &str,
        version_id: &str,
        is_mod: bool,
    ) -> Option<bool> {
        let query = version_query(target, is_mod)?;
        let resp = client
            .get(format!("https://api.modrinth.com/v2/project/{project_id}/version"))
            .query(&query)
            .send()
            .ok()?
            .error_for_status()
            .ok()?;
        let versions = resp.json::<Vec<ModrinthVersion>>().ok()?;
        // Empty after filtering means even the pinned file doesn't match the
        // instance's loader/version: nothing to compare against, so Unknown,
        // not "up to date".
        let latest = versions.first()?;
        Some(latest.id != version_id)
    }

    /// Same three-way logic the old sequential loop used — UpdateAvailable
    /// if any mod needs one, else UpToDate if at least one was successfully
    /// checked, else Unknown — but fans the per-mod network calls out
    /// concurrently (same `thread::scope` + `scope.spawn` + `.join()`
    /// pattern gog.rs already uses for its own one-call-per-item problem)
    /// instead of making them one at a time.
    fn mod_update_status(instance_path: &Path, target: Option<&InstanceTarget>) -> UpdateStatus {
        let Some(target) = target else {
            return UpdateStatus::Unknown;
        };
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

        let checks: Vec<(String, String, bool)> = index
            .files
            .iter()
            .filter_map(|file| {
                let (project, version) = Self::parse_modrinth_url(file.downloads.first()?)?;
                Some((project, version, file.path.starts_with("mods/")))
            })
            .collect();

        // Bounded batches instead of one thread + request per mod at once:
        // a large pack used to fire hundreds of simultaneous requests, which
        // a rate limiter answers with errors (counted as failed checks).
        let mut results: Vec<Option<bool>> = Vec::with_capacity(checks.len());
        for batch in checks.chunks(MAX_CONCURRENT_MOD_CHECKS) {
            let batch_results: Vec<Option<bool>> = std::thread::scope(|scope| {
                let handles: Vec<_> = batch
                    .iter()
                    .map(|(project_id, version_id, is_mod)| {
                        scope.spawn(|| Self::check_one_mod(&client, target, project_id, version_id, *is_mod))
                    })
                    .collect();
                handles.into_iter().map(|h| h.join().unwrap_or(None)).collect()
            });
            results.extend(batch_results);
        }
        status_from_mod_checks(&results)
    }
}

/// At most this many Modrinth requests in flight at once per instance.
const MAX_CONCURRENT_MOD_CHECKS: usize = 8;

/// Combines per-mod results (`Some(true)` = newer version exists,
/// `Some(false)` = current, `None` = could not be checked). A confirmed newer
/// version is a real finding even if other checks failed. "Up to date" is
/// claimed only when EVERY mod was checked successfully — one success among
/// many failures proves nothing about the rest, so that is `Unknown`.
fn status_from_mod_checks(results: &[Option<bool>]) -> UpdateStatus {
    if results.contains(&Some(true)) {
        UpdateStatus::UpdateAvailable
    } else if !results.is_empty() && results.iter().all(|r| *r == Some(false)) {
        UpdateStatus::UpToDate
    } else {
        UpdateStatus::Unknown
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
            ..Game::default()
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
            let target = Self::instance_target(&path);
            let installed_version = target.as_ref().map(|t| t.minecraft.clone());
            let status = Self::mod_update_status(&path, target.as_ref());

            games.push(Game {
                launcher: "prismlauncher",
                id: dir_name,
                name,
                installed_build: installed_version,
                status,
                // lastLaunchTime is set to "now" (ms since epoch) each time the
                // instance starts — BaseInstance::setRunning in
                // PrismLauncher's launcher/BaseInstance.cpp (read from the
                // source this session). 0 = never launched (the setting's
                // default), reported as None. Previously this value was
                // mislabelled `last_updated`; that field is no longer set here.
                last_played: last_played_ms.filter(|ms| *ms != 0).map(|ms| ms / 1000),
                ..Game::default()
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
        super::spawn_exe(&exe, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FABRIC_PACK: &str = r#"{"components":[
        {"cachedVersion":"3.3.1","uid":"org.lwjgl3","version":"3.3.1"},
        {"cachedVersion":"1.20.1","uid":"net.minecraft","version":"1.20.1"},
        {"cachedVersion":"1.20.1","uid":"net.fabricmc.intermediary","version":"1.20.1"},
        {"cachedVersion":"0.19.5","uid":"net.fabricmc.fabric-loader","version":"0.19.5"}],"formatVersion":1}"#;

    #[test]
    fn instance_target_reads_fabric_and_minecraft_version() {
        assert_eq!(
            instance_target(FABRIC_PACK),
            Some(InstanceTarget { minecraft: "1.20.1".to_string(), loader: Some("fabric") })
        );
    }

    #[test]
    fn instance_target_maps_other_loader_uids_and_vanilla() {
        let pack = |uid: &str| format!(r#"{{"components":[{{"cachedVersion":"1.21","uid":"net.minecraft"}},{{"uid":"{uid}"}}]}}"#);
        assert_eq!(instance_target(&pack("org.quiltmc.quilt-loader")).unwrap().loader, Some("quilt"));
        assert_eq!(instance_target(&pack("net.minecraftforge")).unwrap().loader, Some("forge"));
        assert_eq!(instance_target(&pack("net.neoforged")).unwrap().loader, Some("neoforge"));
        assert_eq!(instance_target(&pack("org.lwjgl3")).unwrap().loader, None);
    }

    #[test]
    fn instance_target_falls_back_to_version_and_rejects_missing_minecraft() {
        let pinned = r#"{"components":[{"uid":"net.minecraft","version":"1.19.4"}]}"#;
        assert_eq!(instance_target(pinned).unwrap().minecraft, "1.19.4");
        assert_eq!(instance_target(r#"{"components":[]}"#), None);
        assert_eq!(instance_target("not json"), None);
    }

    #[test]
    fn version_query_filters_mods_by_loader_and_game_version() {
        let t = instance_target(FABRIC_PACK).unwrap();
        let q = version_query(&t, true).unwrap();
        assert!(q.contains(&("loaders", r#"["fabric"]"#.to_string())));
        assert!(q.contains(&("game_versions", r#"["1.20.1"]"#.to_string())));
        assert!(q.contains(&("include_changelog", "false".to_string())));
    }

    #[test]
    fn version_query_non_mod_files_skip_the_loader_filter() {
        let t = instance_target(FABRIC_PACK).unwrap();
        let q = version_query(&t, false).unwrap();
        assert!(q.iter().all(|(k, _)| *k != "loaders"));
        assert!(q.iter().any(|(k, _)| *k == "game_versions"));
    }

    #[test]
    fn version_query_mod_without_known_loader_cannot_be_filtered() {
        let t = InstanceTarget { minecraft: "1.20.1".to_string(), loader: None };
        assert_eq!(version_query(&t, true), None);
        assert!(version_query(&t, false).is_some());
    }

    #[test]
    fn version_query_escapes_odd_minecraft_version_strings() {
        let t = InstanceTarget { minecraft: "1.14 \"x\"".to_string(), loader: Some("fabric") };
        let q = version_query(&t, false).unwrap();
        assert_eq!(q[0].1, r#"["1.14 \"x\""]"#);
    }

    #[test]
    fn mod_checks_all_current_is_up_to_date() {
        assert_eq!(status_from_mod_checks(&[Some(false), Some(false)]), UpdateStatus::UpToDate);
    }

    #[test]
    fn mod_checks_partial_failure_is_unknown_not_up_to_date() {
        // The bug: 1 success among failures used to report "Up to date".
        assert_eq!(status_from_mod_checks(&[Some(false), None, None]), UpdateStatus::Unknown);
    }

    #[test]
    fn mod_checks_a_confirmed_newer_version_wins_even_with_failures() {
        assert_eq!(status_from_mod_checks(&[None, Some(true), Some(false)]), UpdateStatus::UpdateAvailable);
    }

    #[test]
    fn mod_checks_empty_or_all_failed_is_unknown() {
        assert_eq!(status_from_mod_checks(&[]), UpdateStatus::Unknown);
        assert_eq!(status_from_mod_checks(&[None, None]), UpdateStatus::Unknown);
    }

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
