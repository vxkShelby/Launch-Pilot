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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_parses_to_empty_map() {
        assert!(parse("").is_empty());
    }

    #[test]
    fn section_header_and_key_value_pair() {
        let map = parse("[General]\nInstanceDir=instances");
        assert_eq!(map.get("InstanceDir"), Some(&"instances".to_string()));
        // The section header itself is never inserted as a key.
        assert_eq!(map.len(), 1);
    }

    #[test]
    fn value_containing_equals_only_splits_on_the_first_one() {
        let map = parse("path=C:\\x=y\\z");
        assert_eq!(map.get("path"), Some(&"C:\\x=y\\z".to_string()));
    }

    #[test]
    fn comment_line_is_ignored() {
        let map = parse(";this is a comment\nname=value");
        assert_eq!(map.len(), 1);
        assert_eq!(map.get("name"), Some(&"value".to_string()));
    }

    #[test]
    fn blank_lines_are_ignored() {
        let map = parse("\n\nname=value\n\n");
        assert_eq!(map.len(), 1);
        assert_eq!(map.get("name"), Some(&"value".to_string()));
    }

    #[test]
    fn whitespace_padded_key_and_value_are_trimmed() {
        let map = parse("  name  =  value  ");
        assert_eq!(map.get("name"), Some(&"value".to_string()));
    }
}
