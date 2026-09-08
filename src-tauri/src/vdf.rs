// Minimal parser for Valve's VDF/KeyValues format, as used in libraryfolders.vdf
// and appmanifest_*.acf. Only handles what Steam actually writes: quoted
// string keys/values and nested `{ }` blocks, no comments or conditionals
// (those exist in the full KeyValues spec but Steam doesn't emit them here).
// Source: observed directly from a real Steam install's steamapps/ folder.
use std::collections::HashMap;

pub enum Value {
    Str(String),
    Block(HashMap<String, Value>),
}

impl Value {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            Value::Block(_) => None,
        }
    }

    pub fn as_block(&self) -> Option<&HashMap<String, Value>> {
        match self {
            Value::Block(b) => Some(b),
            Value::Str(_) => None,
        }
    }
}

pub fn parse(input: &str) -> Option<HashMap<String, Value>> {
    let mut chars = input.chars().peekable();
    parse_block(&mut chars)
}

fn parse_block(chars: &mut std::iter::Peekable<std::str::Chars>) -> Option<HashMap<String, Value>> {
    let mut map = HashMap::new();
    loop {
        skip_whitespace(chars);
        match chars.peek() {
            None | Some('}') => {
                chars.next();
                return Some(map);
            }
            Some('"') => {
                let key = parse_string(chars)?;
                skip_whitespace(chars);
                match chars.peek() {
                    Some('"') => {
                        let value = parse_string(chars)?;
                        map.insert(key, Value::Str(value));
                    }
                    Some('{') => {
                        chars.next();
                        let block = parse_block(chars)?;
                        map.insert(key, Value::Block(block));
                    }
                    _ => return None,
                }
            }
            _ => {
                chars.next();
            }
        }
    }
}

fn parse_string(chars: &mut std::iter::Peekable<std::str::Chars>) -> Option<String> {
    chars.next(); // opening quote
    let mut s = String::new();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(s),
            // KeyValues escapes the next character verbatim (\" for a
            // literal quote, \\ for a literal backslash — Steam's own
            // paths like "X:\\Steam" rely on the latter).
            '\\' => {
                if let Some(escaped) = chars.next() {
                    s.push(escaped);
                } else {
                    return None;
                }
            }
            _ => s.push(c),
        }
    }
    None
}

fn skip_whitespace(chars: &mut std::iter::Peekable<std::str::Chars>) {
    while matches!(chars.peek(), Some(c) if c.is_whitespace()) {
        chars.next();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_parses_to_empty_map() {
        assert!(parse("").unwrap().is_empty());
    }

    #[test]
    fn flat_key_values() {
        let map = parse(r#""appid"		"1018830"
"name"		"Element TD 2""#)
        .unwrap();
        assert_eq!(map.get("appid").unwrap().as_str(), Some("1018830"));
        assert_eq!(map.get("name").unwrap().as_str(), Some("Element TD 2"));
    }

    #[test]
    fn nested_blocks() {
        let map = parse(r#""AppState" { "appid" "1" "InstalledDepots" { "2" { "size" "3" } } }"#).unwrap();
        let app_state = map.get("AppState").unwrap().as_block().unwrap();
        assert_eq!(app_state.get("appid").unwrap().as_str(), Some("1"));
        let depots = app_state.get("InstalledDepots").unwrap().as_block().unwrap();
        let depot = depots.get("2").unwrap().as_block().unwrap();
        assert_eq!(depot.get("size").unwrap().as_str(), Some("3"));
    }

    #[test]
    fn escaped_backslash_in_value_decodes_to_one_backslash() {
        let map = parse(r#""path"		"X:\\Steam""#).unwrap();
        assert_eq!(map.get("path").unwrap().as_str(), Some("X:\\Steam"));
    }

    #[test]
    fn escaped_quote_in_value_does_not_truncate_string() {
        let map = parse(r#""name"		"Say \"hi\"""#).unwrap();
        assert_eq!(map.get("name").unwrap().as_str(), Some("Say \"hi\""));
    }

    #[test]
    fn unterminated_string_returns_none() {
        assert!(parse(r#""key"		"unterminated"#).is_none());
    }

    #[test]
    fn deeply_nested_blocks_do_not_overflow() {
        let depth = 500;
        let input = "\"a\"".to_string() + &" { \"a\"".repeat(depth) + &" \"1\"" + &" }".repeat(depth);
        assert!(parse(&input).is_some());
    }
}
