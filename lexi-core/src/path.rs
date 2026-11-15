//! Dotted/bracketed path access over `serde_json::Value`.
//!
//! Mirrors `lexi_cli.api_compose` path helpers: `"a.b[0].c"` -> `['a','b',0,'c']`.

use serde_json::{Map, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Seg {
    Key(String),
    Index(usize),
}

/// Parse a dotted/bracketed path into segments.
pub fn parse_path(path: &str) -> Vec<Seg> {
    let mut segs = Vec::new();
    let bytes = path.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i] as char;
        if c == '[' {
            let mut j = i + 1;
            let mut num = String::new();
            while j < bytes.len() && (bytes[j] as char).is_ascii_digit() {
                num.push(bytes[j] as char);
                j += 1;
            }
            if j < bytes.len() && bytes[j] as char == ']' && !num.is_empty() {
                segs.push(Seg::Index(num.parse().unwrap()));
                i = j + 1;
            } else {
                // Malformed bracket: skip the '[' and continue (lenient).
                i += 1;
            }
        } else if c == '.' || c == ']' {
            i += 1;
        } else {
            let mut key = String::new();
            while i < bytes.len() {
                let ch = bytes[i] as char;
                if ch == '.' || ch == '[' || ch == ']' {
                    break;
                }
                key.push(ch);
                i += 1;
            }
            if !key.is_empty() {
                segs.push(Seg::Key(key));
            }
        }
    }
    segs
}

/// Walk a path and return a reference to the value, or `None` on any miss.
pub fn get_path<'a>(obj: &'a Value, path: &str) -> Option<&'a Value> {
    let mut cur = obj;
    for seg in parse_path(path) {
        match seg {
            Seg::Index(i) => match cur {
                Value::Array(a) if i < a.len() => cur = &a[i],
                _ => return None,
            },
            Seg::Key(k) => match cur {
                Value::Object(m) => cur = m.get(&k)?,
                _ => return None,
            },
        }
    }
    Some(cur)
}

fn ensure_container(v: &mut Value, want_array: bool) {
    if want_array {
        if !v.is_array() {
            *v = Value::Array(Vec::new());
        }
    } else if !v.is_object() {
        *v = Value::Object(Map::new());
    }
}

/// Set `value` at `path`, creating intermediate objects/arrays as needed.
pub fn set_path(obj: &mut Value, path: &str, value: Value) {
    let segs = parse_path(path);
    if segs.is_empty() {
        return;
    }
    set_segs(obj, &segs, value);
}

fn set_segs(cur: &mut Value, segs: &[Seg], value: Value) {
    let (head, rest) = segs.split_first().unwrap();
    if rest.is_empty() {
        match head {
            Seg::Key(k) => {
                ensure_container(cur, false);
                cur.as_object_mut().unwrap().insert(k.clone(), value);
            }
            Seg::Index(i) => {
                ensure_container(cur, true);
                let a = cur.as_array_mut().unwrap();
                while a.len() <= *i {
                    a.push(Value::Null);
                }
                a[*i] = value;
            }
        }
        return;
    }
    let next_is_index = matches!(rest[0], Seg::Index(_));
    match head {
        Seg::Key(k) => {
            ensure_container(cur, false);
            let child = cur
                .as_object_mut()
                .unwrap()
                .entry(k.clone())
                .or_insert(Value::Null);
            ensure_container(child, next_is_index);
            set_segs(child, rest, value);
        }
        Seg::Index(i) => {
            ensure_container(cur, true);
            let a = cur.as_array_mut().unwrap();
            while a.len() <= *i {
                a.push(Value::Null);
            }
            let child = &mut a[*i];
            ensure_container(child, next_is_index);
            set_segs(child, rest, value);
        }
    }
}

/// Remove the value at `path`. No-op if the path does not exist.
pub fn unset_path(obj: &mut Value, path: &str) {
    let segs = parse_path(path);
    if segs.is_empty() {
        return;
    }
    let mut cur = obj;
    for seg in &segs[..segs.len() - 1] {
        match seg {
            Seg::Index(i) => match cur {
                Value::Array(a) if *i < a.len() => cur = &mut a[*i],
                _ => return,
            },
            Seg::Key(k) => match cur {
                Value::Object(m) if m.contains_key(k) => cur = m.get_mut(k).unwrap(),
                _ => return,
            },
        }
    }
    match segs.last().unwrap() {
        Seg::Index(i) => {
            if let Value::Array(a) = cur {
                if *i < a.len() {
                    a.remove(*i);
                }
            }
        }
        Seg::Key(k) => {
            if let Value::Object(m) = cur {
                m.remove(k);
            }
        }
    }
}
