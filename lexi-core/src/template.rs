//! Template rendering with `{{var}}` substitution.
//!
//! Mirrors `lexi_cli.api_compose.render_template`: a string that is *exactly*
//! `"{{var}}"` is replaced with the raw binding value (type preserved); an
//! embedded placeholder is substituted with the stringified value.

use serde_json::{Map, Value};

/// Python-ish `str()` for scalars, used only for *embedded* substitution
/// (the exact-match path returns the raw value and never reaches this).
fn py_str(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Bool(b) => {
            if *b {
                "True".to_string()
            } else {
                "False".to_string()
            }
        }
        Value::Null => "None".to_string(),
        Value::Number(n) => n.to_string(),
        other => other.to_string(),
    }
}

/// Substitute every `{{name}}` occurrence in `s` (embedded case).
/// UTF-8 safe: literal segments are copied as `&str` slices.
fn substitute_embedded(s: &str, vars: &Map<String, Value>) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(open) = rest.find("{{") {
        out.push_str(&rest[..open]);
        let after = &rest[open + 2..];
        match after.find("}}") {
            Some(close) => {
                let inner = &after[..close];
                let key = inner.trim();
                if key.is_empty() {
                    // Python's `.+?` requires >=1 char; leave "{{" literal.
                    out.push_str("{{");
                    rest = after;
                } else {
                    match vars.get(key) {
                        Some(v) => out.push_str(&py_str(v)),
                        None => {
                            out.push_str("{{");
                            out.push_str(inner);
                            out.push_str("}}");
                        }
                    }
                    rest = &after[close + 2..];
                }
            }
            None => {
                out.push_str("{{");
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Recursively render a template against `vars`.
pub fn render_template(template: &Value, vars: &Map<String, Value>) -> Value {
    match template {
        Value::String(s) => {
            let stripped = s.trim();
            if stripped.starts_with("{{") && stripped.ends_with("}}") && stripped.len() >= 4 {
                let key = stripped[2..stripped.len() - 2].trim();
                if let Some(v) = vars.get(key) {
                    return v.clone();
                }
            }
            Value::String(substitute_embedded(s, vars))
        }
        Value::Object(m) => {
            let mut out = Map::new();
            for (k, v) in m {
                out.insert(k.clone(), render_template(v, vars));
            }
            Value::Object(out)
        }
        Value::Array(a) => Value::Array(a.iter().map(|v| render_template(v, vars)).collect()),
        other => other.clone(),
    }
}
