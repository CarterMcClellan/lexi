//! `config` — the CLI's app settings (`config.json`).

use serde_json::{json, Map, Value};

use crate::cli::ConfigCmd;
use crate::output::{pretty, scalar, Io};

/// Settings every install has, overlaid by `config.json` (as the Python CLI does).
pub(crate) const CONFIG_DEFAULTS: &[(&str, &str)] = &[
    ("cli_name", "lexi"),
    ("prompt", "cmd"),
    ("prompt_delimiter", ">"),
    ("edit_mode", "vi"),
];

/// Defaults overlaid with the saved `config.json`.
pub(crate) fn effective_config() -> Result<Map<String, Value>, lexi_host::HostError> {
    let mut merged: Map<String, Value> = CONFIG_DEFAULTS
        .iter()
        .map(|(k, v)| (k.to_string(), Value::from(*v)))
        .collect();
    if let Value::Object(saved) = lexi_host::config::load_config()? {
        merged.extend(saved);
    }
    merged.remove("version"); // legacy key; the version comes from the binary
    Ok(merged)
}

pub(crate) fn run(io: &mut Io<'_>, cmd: ConfigCmd) -> u8 {
    match cmd {
        ConfigCmd::List { key, raw, value } => list(io, key, raw, value),
    }
}

fn list(io: &mut Io<'_>, key: Option<String>, raw: bool, value_only: bool) -> u8 {
    let config = match effective_config() {
        Ok(c) => c,
        Err(e) => return io.fail(e),
    };
    if let Some(key) = key {
        let Some(v) = config.get(&key) else {
            return io.fail(format_args!("config key {key:?} not found"));
        };
        let human = if value_only {
            scalar(Some(v))
        } else {
            format!("{key}=\"{}\"", scalar(Some(v)))
        };
        return io.result(human, json!({ "key": key, "value": v }));
    }
    if raw {
        let text = super::read_home_file("config.json").unwrap_or_else(|_| "{}".into());
        let human = serde_json::from_str::<Value>(&text)
            .map(|v| pretty(&v))
            .unwrap_or(text);
        return io.result(human, Value::Object(config));
    }
    let human = if config.is_empty() {
        "No config values set.".to_string()
    } else {
        config
            .iter()
            .map(|(k, v)| format!("{k}=\"{}\"", scalar(Some(v))))
            .collect::<Vec<_>>()
            .join("\n")
    };
    io.result(human, Value::Object(config))
}
