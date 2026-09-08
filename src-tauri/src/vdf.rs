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
    for c in chars.by_ref() {
        if c == '"' {
            return Some(s);
        }
        s.push(c);
    }
    None
}

fn skip_whitespace(chars: &mut std::iter::Peekable<std::str::Chars>) {
    while matches!(chars.peek(), Some(c) if c.is_whitespace()) {
        chars.next();
    }
}
