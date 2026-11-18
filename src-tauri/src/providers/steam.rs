// Steam reference provider.
//
// Detection: SteamPath from HKCU\Software\Valve\Steam (written by the Steam
// client itself on every launch).
// Library discovery: <SteamPath>/steamapps/libraryfolders.vdf lists every
// library folder as a numbered block with a "path" key.
// Game enumeration: each library's steamapps/appmanifest_<appid>.acf holds
// one flat "AppState" block per installed game.
// Update detection: Steam's own "StateFlags" bitfield (values verified
// against a real install, not guessed):
//   1=Uninstalled 2=UpdateRequired 4=FullyInstalled 8=Encrypted 16=Locked
//   32=FilesMissing 64=AppRunning 128=FilesCorrupt
// The "actively downloading right now" bit is 1024, not the 256 this file
// previously assumed (an older, unverified guess) — corrected after a real
// user's live StateFlags dump showed every merely-queued pending update at
// StateFlags=6 (2|4) while the two games actually mid-download that moment
// both read 1030 (2|4|1024), confirmed live against Steam's own progress
// bars for those exact two titles (RoadCraft, War Thunder) on 2026-09-19.
// A game also carries "buildid" (installed) vs "TargetBuildID" (what Steam
// wants installed next); TargetBuildID is "0" when Steam hasn't queued one.
use super::{Game, LauncherProvider, UpdateStatus};
use crate::steam_common;
use crate::vdf;
use std::path::{Path, PathBuf};

const STATE_UPDATE_REQUIRED: u64 = 2;
const STATE_FULLY_INSTALLED: u64 = 4;
const STATE_UPDATE_RUNNING: u64 = 1024;

/// Standalone so it can be unit-tested directly against raw StateFlags/
/// buildid combinations without needing a real manifest file on disk — this
/// exact decision has already shipped two real bugs (see file-top comment),
/// both caught only via a live user bug report, so it gets its own tests.
///
/// Bit 1024 means Steam is actively streaming bytes for this game right
/// now — checked before UpdateRequired (2), which every observed
/// mid-download game also carried (both bits set together: 1030 =
/// 1024|4|2). Real bug fixed here, reported by a user: an earlier version
/// of this check tested bit 256 instead, a guess that turned out wrong — a
/// live StateFlags dump from that user's own machine (see file-top comment)
/// never showed 256 anywhere, and showed 1024 set on exactly the two games
/// actively downloading at that moment (confirmed against Steam's own
/// progress bars for those two), with every other pending-but-queued game
/// at a plain 6.
fn derive_status(flags: u64, buildid: Option<&str>, target_buildid: Option<&str>) -> UpdateStatus {
    if flags & STATE_UPDATE_RUNNING != 0 {
        UpdateStatus::Updating
    } else if flags & STATE_UPDATE_REQUIRED != 0 {
        UpdateStatus::UpdateAvailable
    } else if let (Some(b), Some(t)) = (buildid, target_buildid) {
        if t != "0" && t != b {
            UpdateStatus::UpdateAvailable
        } else if flags & STATE_FULLY_INSTALLED != 0 {
            UpdateStatus::UpToDate
        } else {
            UpdateStatus::Unknown
        }
    } else if flags & STATE_FULLY_INSTALLED != 0 {
        UpdateStatus::UpToDate
    } else {
        UpdateStatus::Unknown
    }
}

/// Reads a manifest field as a number. Steam writes every value as a quoted
/// string; a missing or non-numeric key is `None`.
fn num(state: &std::collections::HashMap<String, vdf::Value>, key: &str) -> Option<u64> {
    state.get(key).and_then(|v| v.as_str()).and_then(|s| s.parse().ok())
}

/// Pure manifest -> `Game` step (no file I/O) so the field handling can be
/// tested against manifest-shaped text.
///
/// Pending-update numbers (`BytesToDownload`, `BytesDownloaded`,
/// `ScheduledAutoUpdate`) are only reported while the status is
/// `UpdateAvailable`/`Updating`: on every idle manifest checked on the
/// reference machine `BytesToDownload == BytesDownloaded` and non-zero, i.e.
/// the *previous* update's totals are left behind, so reading them
/// unconditionally would show a stale size on an up-to-date game.
/// Semantics are inferred from observed data, not from Valve documentation:
/// on the one live pending game seen (appid 730, StateFlags 6) these read
/// BytesToDownload=4869745696, BytesDownloaded=0 and a ScheduledAutoUpdate
/// in the future. Values mid-download (StateFlags 1030) were not observed
/// first-hand.
///
/// `LastPlayed` / `ScheduledAutoUpdate` are 0 when unset, which is reported
/// as `None` (not "never"/"now").
fn game_from_manifest(content: &str) -> Option<Game> {
    let root = vdf::parse(content)?;
    let state = root.get("AppState")?.as_block()?;

    let id = state.get("appid")?.as_str()?.to_string();
    let name = state.get("name")?.as_str()?.to_string();
    let flags = num(state, "StateFlags").unwrap_or(0);
    let buildid = state.get("buildid").and_then(|v| v.as_str());
    let target_buildid = state.get("TargetBuildID").and_then(|v| v.as_str());

    let status = derive_status(flags, buildid, target_buildid);
    let pending = matches!(status, UpdateStatus::UpdateAvailable | UpdateStatus::Updating);
    let nonzero = |v: Option<u64>| v.filter(|n| *n != 0);
    let to_download = if pending { nonzero(num(state, "BytesToDownload")) } else { None };

    Some(Game {
        launcher: "steam",
        id,
        name,
        installed_build: buildid.map(String::from),
        status,
        size_bytes: num(state, "SizeOnDisk"),
        last_updated: num(state, "LastUpdated"),
        last_played: nonzero(num(state, "LastPlayed")),
        update_bytes_total: to_download,
        update_bytes_done: if to_download.is_some() { num(state, "BytesDownloaded") } else { None },
        update_scheduled: if pending { nonzero(num(state, "ScheduledAutoUpdate")) } else { None },
        ..Game::default()
    })
}

pub struct SteamProvider;

impl SteamProvider {
    fn steam_path(&self) -> Option<PathBuf> {
        steam_common::steam_path()
    }

    fn library_paths(&self, steam_path: &Path) -> Vec<PathBuf> {
        steam_common::library_paths(steam_path)
    }

    fn parse_manifest(&self, path: &Path) -> Option<Game> {
        game_from_manifest(&std::fs::read_to_string(path).ok()?)
    }
}

impl LauncherProvider for SteamProvider {
    fn id(&self) -> &'static str {
        "steam"
    }

    fn display_name(&self) -> &'static str {
        "Steam"
    }

    fn detect(&self) -> bool {
        self.steam_path().is_some_and(|p| p.join("steam.exe").exists())
    }

    fn list_games(&self) -> Result<Vec<Game>, String> {
        let steam_path = self.steam_path().ok_or("Steam not installed")?;
        let mut games = Vec::new();

        for library in self.library_paths(&steam_path) {
            let steamapps = library.join("steamapps");
            let Ok(entries) = std::fs::read_dir(&steamapps) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                let is_manifest = path
                    .file_name()
                    .and_then(|f| f.to_str())
                    .is_some_and(|f| f.starts_with("appmanifest_") && f.ends_with(".acf"));
                if is_manifest {
                    if let Some(game) = self.parse_manifest(&path) {
                        games.push(game);
                    }
                }
            }
        }
        Ok(games)
    }

    fn process_names(&self) -> &'static [&'static str] {
        &["steam.exe"]
    }

    fn icon_source(&self) -> Option<PathBuf> {
        Some(self.steam_path()?.join("steam.exe"))
    }

    // Steam's own manifest gives an installdir but never a launch exe
    // (that lives in localconfig.vdf's launch options, a much messier
    // per-user file) — same disclosed "matching/largest exe" heuristic
    // used for Ubisoft, applied to the real install folder.
    fn game_icon_source(&self, game_id: &str) -> Option<PathBuf> {
        let steam_path = self.steam_path()?;
        for library in self.library_paths(&steam_path) {
            let manifest = library.join("steamapps").join(format!("appmanifest_{game_id}.acf"));
            let Ok(content) = std::fs::read_to_string(&manifest) else {
                continue;
            };
            let Some(root) = crate::vdf::parse(&content) else {
                continue;
            };
            let Some(installdir) = root
                .get("AppState")
                .and_then(|v| v.as_block())
                .and_then(|s| s.get("installdir"))
                .and_then(|v| v.as_str())
            else {
                continue;
            };
            let game_dir = library.join("steamapps").join("common").join(installdir);
            if let Some(exe) = super::find_main_exe(&game_dir) {
                return Some(exe);
            }
        }
        None
    }

    /// The library folder whose `steamappsppmanifest_<id>.acf` exists:
    /// Steam downloads the update into that same library's `steamapps`.
    fn install_root(&self, game_id: &str) -> Option<PathBuf> {
        super::uri_safe_id(game_id).ok()?;
        let steam_path = self.steam_path()?;
        self.library_paths(&steam_path)
            .into_iter()
            .find(|lib| lib.join("steamapps").join(format!("appmanifest_{game_id}.acf")).is_file())
    }

    fn trigger_update(&self, game_id: &str) -> Result<(), String> {
        // Empty id = the launcher-level "Open" button: start the Steam client
        // itself (the exe detect() already checks for) instead of opening a
        // meaningless steam://validate/ URI.
        if game_id.is_empty() {
            let exe = self.steam_path().ok_or("Steam not installed")?.join("steam.exe");
            return super::spawn_exe(&exe, None);
        }
        // No public "force update" API exists for Steam. steam://validate
        // is Valve's own documented mechanism for re-verifying a game's
        // files, which re-downloads anything missing or out of date.
        let id = super::uri_safe_id(game_id)?;
        open::that(format!("steam://validate/{id}")).map_err(|e| e.to_string())
    }

    // steam://rungameid/<appid> is Valve's own documented protocol — the
    // exact mechanism Steam's own store-page "Play" button uses, so Steam
    // itself resolves the real launch target instead of LaunchPilot
    // guessing one. This replaces an earlier exe-spawn approach (via
    // game_icon_source/find_main_exe) that crashed live on this machine:
    // 3DMark's manifest carries "InstallScripts" (Steam's signal for a
    // custom install wrapper) and has no single main exe, so the heuristic
    // picked a Steam-invoked setup stub (SystemInfoSetupAssistant.exe) that
    // crashes with STATUS_ACCESS_VIOLATION (0xC0000005) run standalone.
    // rungameid hands the problem to Steam, which already knows how to
    // launch any of its games correctly — install-script/multi-exe games
    // included — so no InstallScripts guard is needed here anymore. Same
    // `open` crate + protocol-URI pattern already used by trigger_update.
    fn launch(&self, game_id: &str) -> Result<(), String> {
        let id = super::uri_safe_id(game_id)?;
        open::that(format!("steam://rungameid/{id}")).map_err(|e| e.to_string())
    }

    fn supports_launch(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Manifest-shaped fixture: field names, order and tab layout copied
    /// from real appmanifest files on the reference machine. The numbers in
    /// the pending test are the ones read from appid 730 (StateFlags 6).
    /// The mid-download test (flags 1030) reuses the shape with invented
    /// progress numbers — synthetic, there only to exercise the parser.
    fn manifest(flags: u32, buildid: u64, target: u64, to_dl: u64, dl: u64, sched: u64, last_played: u64) -> String {
        format!(
            "\"AppState\"\n{{\n\t\"appid\"\t\t\"730\"\n\t\"name\"\t\t\"Counter-Strike 2\"\n\t\"StateFlags\"\t\t\"{flags}\"\n\t\"LastUpdated\"\t\t\"1794469273\"\n\t\"LastPlayed\"\t\t\"{last_played}\"\n\t\"SizeOnDisk\"\t\t\"15014558867\"\n\t\"buildid\"\t\t\"{buildid}\"\n\t\"BytesToDownload\"\t\t\"{to_dl}\"\n\t\"BytesDownloaded\"\t\t\"{dl}\"\n\t\"TargetBuildID\"\t\t\"{target}\"\n\t\"ScheduledAutoUpdate\"\t\t\"{sched}\"\n}}\n"
        )
    }

    #[test]
    fn pending_manifest_exposes_download_size_and_schedule() {
        let g = game_from_manifest(&manifest(6, 26598758, 26748068, 4869745696, 0, 1795929385, 1793305048)).unwrap();
        assert_eq!(g.status, UpdateStatus::UpdateAvailable);
        assert_eq!(g.update_bytes_total, Some(4869745696));
        assert_eq!(g.update_bytes_done, Some(0));
        assert_eq!(g.update_scheduled, Some(1795929385));
        assert_eq!(g.last_played, Some(1793305048));
        assert_eq!(g.last_updated, Some(1794469273));
        assert_eq!(g.size_bytes, Some(15014558867));
    }

    #[test]
    fn mid_download_manifest_reports_progress() {
        let g = game_from_manifest(&manifest(1030, 100, 101, 5000, 1200, 0, 0)).unwrap();
        assert_eq!(g.status, UpdateStatus::Updating);
        assert_eq!(g.update_bytes_total, Some(5000));
        assert_eq!(g.update_bytes_done, Some(1200));
        assert_eq!(g.update_scheduled, None); // 0 = nothing scheduled
        assert_eq!(g.last_played, None); // 0 = unset, not "never played"
    }

    #[test]
    fn idle_manifest_hides_leftover_update_numbers() {
        // Real idle manifests keep BytesToDownload == BytesDownloaded from the
        // previous update; they must not surface on an up-to-date game.
        let g = game_from_manifest(&manifest(4, 26504054, 26504054, 3444272, 3444272, 0, 1752520007)).unwrap();
        assert_eq!(g.status, UpdateStatus::UpToDate);
        assert_eq!(g.update_bytes_total, None);
        assert_eq!(g.update_bytes_done, None);
        assert_eq!(g.update_scheduled, None);
        assert_eq!(g.last_played, Some(1752520007));
    }

    #[test]
    fn pending_with_zero_bytes_to_download_reports_no_size() {
        let g = game_from_manifest(&manifest(6, 1, 2, 0, 0, 0, 0)).unwrap();
        assert_eq!(g.status, UpdateStatus::UpdateAvailable);
        assert_eq!(g.update_bytes_total, None);
        assert_eq!(g.update_bytes_done, None);
    }

    #[test]
    fn manifest_without_appstate_is_rejected() {
        assert!(game_from_manifest("\"Other\"\n{\n}\n").is_none());
    }

    #[test]
    fn actively_downloading_flags_report_updating() {
        // Real observed case from the bug history: 1024|4|2, both games
        // actively mid-download the moment this was captured live.
        assert_eq!(derive_status(1030, None, None), UpdateStatus::Updating);
    }

    #[test]
    fn queued_only_flags_report_update_available() {
        // 2|4: UpdateRequired set but not yet actively downloading.
        assert_eq!(derive_status(6, None, None), UpdateStatus::UpdateAvailable);
    }

    #[test]
    fn buildid_mismatch_reports_update_available_independent_of_flags() {
        // UpdateRequired bit (2) not set at all here — only the buildid
        // path should catch this.
        assert_eq!(derive_status(4, Some("5"), Some("7")), UpdateStatus::UpdateAvailable);
    }

    #[test]
    fn target_buildid_zero_means_no_pending_build_reports_up_to_date() {
        // TargetBuildID "0" is Steam's own way of saying "nothing queued".
        assert_eq!(derive_status(4, Some("5"), Some("0")), UpdateStatus::UpToDate);
    }

    #[test]
    fn no_flags_and_no_buildid_info_is_unknown() {
        assert_eq!(derive_status(0, None, None), UpdateStatus::Unknown);
    }

    #[test]
    fn matching_buildids_with_fully_installed_flag_report_up_to_date() {
        assert_eq!(derive_status(4, Some("5"), Some("5")), UpdateStatus::UpToDate);
    }
}
