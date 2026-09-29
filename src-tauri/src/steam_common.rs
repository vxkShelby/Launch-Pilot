// Shared Steam-install helpers used by both steam.rs (native Steam games)
// and ubisoft.rs (Steam-cross-detected Ubisoft games, see ubisoft.rs's own
// top-of-file comment). Extracted because both providers need to find
// Steam's own install path and enumerate its library folders in exactly the
// same way — this was previously duplicated byte-for-byte in both files.
//
// steam_path(): reads SteamPath from HKCU\Software\Valve\Steam (written by
// the Steam client itself on every launch).
// library_paths(): parses <SteamPath>/steamapps/libraryfolders.vdf, which
// lists every Steam library folder as a numbered block with a "path" key.
use crate::vdf;
use std::path::{Path, PathBuf};

pub fn steam_path() -> Option<PathBuf> {
    let hkcu = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER);
    let key = hkcu.open_subkey("Software\\Valve\\Steam").ok()?;
    let path: String = key.get_value("SteamPath").ok()?;
    Some(PathBuf::from(path.replace('/', "\\")))
}

pub fn library_paths(steam_path: &Path) -> Vec<PathBuf> {
    let vdf_path = steam_path.join("steamapps").join("libraryfolders.vdf");
    let Ok(content) = std::fs::read_to_string(&vdf_path) else {
        return vec![steam_path.to_path_buf()];
    };
    let Some(root) = vdf::parse(&content) else {
        return vec![steam_path.to_path_buf()];
    };
    let Some(folders) = root.get("libraryfolders").and_then(|v| v.as_block()) else {
        return vec![steam_path.to_path_buf()];
    };

    let mut paths = Vec::new();
    for entry in folders.values() {
        if let Some(block) = entry.as_block() {
            if let Some(path) = block.get("path").and_then(|v| v.as_str()) {
                paths.push(PathBuf::from(path.replace('\\', "/")));
            }
        }
    }
    if paths.is_empty() {
        paths.push(steam_path.to_path_buf());
    }
    paths
}
