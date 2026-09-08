// Minimal flat INI reader for PrismLauncher's config files (prismlauncher.cfg,
// instance.cfg). Both are plain `[Section]` headers + `key=value` lines, no
// nesting, no quoting — verified directly against real files on disk.
// Section headers are ignored; callers just look up keys by name, which is
// safe here because PrismLauncher doesn't reuse key names across sections
// in the fields this app reads.
use std::collections::HashMap;

pub fn parse(input: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in input.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('[') || line.starts_with(';') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            map.insert(key.trim().to_string(), value.trim().to_string());
        }
    }
    map
}
