//! `providers` — the providers in `providers/providers-config.yaml`.

use serde_json::{json, Value};

use crate::cli::ProvidersCmd;
use crate::output::{scalar, table, Io};

pub(crate) fn run(io: &mut Io<'_>, cmd: ProvidersCmd) -> u8 {
    match cmd {
        ProvidersCmd::List { raw, table } => list(io, raw, table),
    }
}

fn list(io: &mut Io<'_>, raw: bool, as_table: bool) -> u8 {
    let cfg = match lexi_host::config::load_providers() {
        Ok(c) => c,
        Err(e) => return io.fail(e),
    };
    let empty = serde_json::Map::new();
    let providers = cfg
        .get("providers")
        .and_then(Value::as_object)
        .unwrap_or(&empty);

    let mut rows = Vec::new();
    for (name, pcfg) in providers {
        let source = match lexi_host::config::api_key_source(name, pcfg) {
            Ok(s) => s,
            Err(e) => return io.fail(e),
        };
        rows.push(json!({
            "name": name,
            "url": pcfg.get("url"),
            "api": pcfg.get("api"),
            "default_model": pcfg.get("default_model"),
            "hosted_models": pcfg.get("hosted_models"),
            "hosted_model_fields": pcfg.get("hosted_model_fields"),
            "key_source": source,
        }));
    }
    let json = Value::Array(rows.clone());

    let human = if raw {
        super::read_home_file("providers/providers-config.yaml").unwrap_or_default()
    } else if rows.is_empty() {
        "No providers configured.".to_string()
    } else if as_table {
        let body = rows
            .iter()
            .map(|r| {
                let status = if r["key_source"].is_null() {
                    "no api-key"
                } else {
                    "ready"
                };
                vec![
                    scalar(r.get("name")),
                    scalar(r.get("url")),
                    scalar(r.get("api")),
                    scalar(r.get("key_source")),
                    status.to_string(),
                ]
                .into_iter()
                .map(|c| if c.is_empty() { "—".to_string() } else { c })
                .collect()
            })
            .collect();
        table(
            "Providers",
            &["Provider", "URL", "API", "Key Source", "Status"],
            body,
        )
    } else {
        rows.iter()
            .map(|r| {
                let fields = r["hosted_model_fields"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .map(|v| scalar(Some(v)))
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
                    .unwrap_or_default();
                let key = match r["key_source"].as_str() {
                    Some(s) => format!("[{s}]"),
                    None => "[missing]".to_string(),
                };
                format!(
                    "{}: url={} api={} hosted={} fields=[{fields}] api_key={key}",
                    scalar(r.get("name")),
                    scalar(r.get("url")),
                    scalar(r.get("api")),
                    scalar(r.get("hosted_models")),
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    io.result(human, json)
}
