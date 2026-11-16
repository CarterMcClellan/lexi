//! Command aliases (`~/.lexi-cli/aliases.json`).
//!
//! The file groups alias names by what they expand to —
//! `{"providers": ["prov"], "exit": ["e", "q"]}` — and the CLI works with the
//! inverse, alias → expansion. (A flat `{"alias": "expansion"}` file is read too.)

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde_json::Value;

fn path() -> PathBuf {
    lexi_host::config::lexi_home().join("aliases.json")
}

/// alias → expansion. Missing or unreadable file: no aliases.
pub(crate) fn load() -> BTreeMap<String, String> {
    let Ok(text) = std::fs::read_to_string(path()) else {
        return BTreeMap::new();
    };
    parse(&text)
}

fn parse(text: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let Ok(Value::Object(data)) = serde_json::from_str::<Value>(text) else {
        return out;
    };
    let grouped = data.values().all(Value::is_array);
    for (k, v) in data {
        if grouped {
            for alias in v.as_array().into_iter().flatten().filter_map(Value::as_str) {
                out.insert(alias.to_string(), k.clone());
            }
        } else {
            out.insert(k, crate::output::scalar(Some(&v)));
        }
    }
    out
}

/// expansion → sorted alias names.
pub(crate) fn grouped(aliases: &BTreeMap<String, String>) -> BTreeMap<String, Vec<String>> {
    let mut g: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (alias, cmd) in aliases {
        g.entry(cmd.clone()).or_default().push(alias.clone());
    }
    for v in g.values_mut() {
        v.sort();
    }
    g
}

pub(crate) fn save(aliases: &BTreeMap<String, String>) -> std::io::Result<()> {
    let text = serde_json::to_string_pretty(&grouped(aliases)).unwrap_or_default();
    std::fs::write(path(), text)
}

pub(crate) fn reset() -> std::io::Result<()> {
    std::fs::write(path(), lexi_host::bootstrap::default_aliases())
}

/// Expand a leading alias in `tokens` (repeatedly, up to a depth limit).
/// Real commands are never shadowed; an alias's expansion may be several words.
pub(crate) fn expand(
    mut tokens: Vec<String>,
    aliases: &BTreeMap<String, String>,
    is_command: impl Fn(&str) -> bool,
) -> Result<Vec<String>, String> {
    for _ in 0..6 {
        let Some(first) = tokens.first() else {
            return Ok(tokens);
        };
        if is_command(first) {
            return Ok(tokens);
        }
        let Some(expansion) = aliases.get(first) else {
            return Ok(tokens);
        };
        let mut expanded = shlex::split(expansion)
            .ok_or_else(|| format!("alias {first:?} has an unparseable expansion"))?;
        expanded.extend(tokens.drain(1..));
        tokens = expanded;
    }
    Err("alias expansion too deep; possible recursion".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_grouped_and_flat_files() {
        let a = parse(r#"{"providers": ["prov", "p"], "exit": ["q"]}"#);
        assert_eq!(a["prov"], "providers");
        assert_eq!(a["q"], "exit");
        let flat = parse(r#"{"pl": "providers list"}"#);
        assert_eq!(flat["pl"], "providers list");
        assert!(parse("{nope").is_empty());
    }

    #[test]
    fn expands_leading_aliases_without_shadowing_commands() {
        let mut a = BTreeMap::new();
        a.insert("pl".to_string(), "providers list".to_string());
        a.insert("x".to_string(), "pl".to_string());
        a.insert("loop".to_string(), "loop".to_string());
        a.insert("models".to_string(), "providers".to_string());
        let cmd = |s: &str| ["providers", "models"].contains(&s);
        let t = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            expand(t(&["x", "-t"]), &a, cmd).unwrap(),
            t(&["providers", "list", "-t"])
        );
        assert_eq!(
            expand(t(&["models", "list"]), &a, cmd).unwrap(),
            t(&["models", "list"])
        );
        assert!(expand(t(&["loop"]), &a, cmd).is_err());
    }
}
