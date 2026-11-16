//! `providers` — the providers in `providers/providers-config.yaml`.

use serde_json::{json, Value};

use crate::cli::{ApiKeyCmd, ProvidersCmd};
use crate::output::{scalar, table, Io};

pub(crate) async fn run(io: &mut Io<'_>, cmd: ProvidersCmd) -> u8 {
    match cmd {
        ProvidersCmd::List { raw, table } => list(io, raw, table),
        ProvidersCmd::Status { name, raw } => status(io, &name, raw).await,
        ProvidersCmd::Add {
            name,
            url,
            default_model,
            api_key,
            api,
            hosted_models,
        } => add(
            io,
            &name,
            &url,
            &default_model,
            api_key.as_deref(),
            &api,
            &hosted_models,
        ),
        ProvidersCmd::ApiKey { cmd } => match cmd {
            ApiKeyCmd::Set { name, value } => key_set(io, &name, value),
            ApiKeyCmd::Show { name, reveal } => key_show(io, &name, reveal),
            ApiKeyCmd::Remove { name } => key_remove(io, &name),
        },
        ProvidersCmd::Rename { old, new } => match lexi_host::config::rename_provider(&old, &new) {
            Ok(()) => io.result(
                format!("Renamed provider {old} -> {new}."),
                json!({ "renamed": old, "to": new }),
            ),
            Err(e) => io.fail(e),
        },
        ProvidersCmd::Rm { name } => match lexi_host::config::remove_provider(&name) {
            Ok(()) => io.result(
                format!("Removed provider {name}."),
                json!({ "removed": name }),
            ),
            Err(e) => io.fail(e),
        },
    }
}

/// The provider's config entry, or fail with "unknown provider".
fn provider_cfg(io: &mut Io<'_>, name: &str) -> Result<Value, u8> {
    let cfg = lexi_host::config::load_providers().map_err(|e| io.fail(e))?;
    cfg.get("providers")
        .and_then(|p| p.get(name))
        .cloned()
        .ok_or_else(|| io.fail(lexi_host::HostError::UnknownProvider(name.into())))
}

async fn status(io: &mut Io<'_>, name: &str, raw: bool) -> u8 {
    match lexi_host::check_api_key(name).await {
        Ok(st) => {
            let json = json!({ "provider": name, "status": st.status, "source": st.source });
            let human = if raw {
                json.to_string()
            } else {
                let source = st.source.map(|s| format!(" ({s})")).unwrap_or_default();
                format!("{name}: {}{source}", st.status)
            };
            io.result(human, json)
        }
        Err(e) => io.fail(e),
    }
}

fn add(
    io: &mut Io<'_>,
    name: &str,
    url: &str,
    default_model: &str,
    api_key: Option<&str>,
    api: &str,
    hosted_models: &str,
) -> u8 {
    if let Err(e) = lexi_host::config::load_api_def(api) {
        return io.fail(e);
    }
    let mut entry = serde_json::Map::new();
    entry.insert("url".into(), Value::from(url));
    entry.insert("hosted_models".into(), Value::from(hosted_models));
    entry.insert("api".into(), Value::from(api));
    entry.insert("default_model".into(), Value::from(default_model));
    if let Err(e) = lexi_host::config::add_provider(name, &entry) {
        return io.fail(e);
    }
    // The key goes to the provider's owner-only key file, never the config.
    if let Some(key) = api_key {
        if let Err(e) = lexi_host::config::set_api_key(name, key) {
            return io.fail(e);
        }
    }
    if let Err(e) = lexi_host::config::set_default_model(name, Some(default_model)) {
        return io.fail(e);
    }
    io.result(format!("Added provider {name}."), json!({ "added": name }))
}

/// Read a key interactively (no echo) or from piped stdin.
fn read_key() -> std::io::Result<String> {
    use std::io::IsTerminal;
    if std::io::stdin().is_terminal() {
        rpassword::prompt_password("Enter API key: ")
    } else {
        let mut line = String::new();
        std::io::stdin().read_line(&mut line)?;
        Ok(line)
    }
}

fn key_set(io: &mut Io<'_>, name: &str, value: Option<String>) -> u8 {
    if let Err(code) = provider_cfg(io, name) {
        return code;
    }
    let key = match value {
        Some(v) => v,
        None => match read_key() {
            Ok(k) => k,
            Err(e) => return io.fail(e),
        },
    };
    match lexi_host::config::set_api_key(name, &key) {
        Ok(()) => io.result(
            format!("Saved API key for {name}."),
            json!({ "provider": name }),
        ),
        Err(e) => io.fail(e),
    }
}

/// `sk-a…wxyz`-style masking: first and last 4 shown when long enough.
fn mask(key: &str) -> String {
    let chars: Vec<char> = key.chars().collect();
    if chars.len() > 8 {
        let head: String = chars[..4].iter().collect();
        let tail: String = chars[chars.len() - 4..].iter().collect();
        format!("{head}{}{tail}", "*".repeat(chars.len() - 8))
    } else {
        "*".repeat(chars.len())
    }
}

fn key_show(io: &mut Io<'_>, name: &str, reveal: bool) -> u8 {
    let cfg = match provider_cfg(io, name) {
        Ok(c) => c,
        Err(code) => return code,
    };
    let source = match lexi_host::config::api_key_source(name, &cfg) {
        Ok(s) => s,
        Err(e) => return io.fail(e),
    };
    if source.is_none() {
        return io.result(
            format!("No API key configured for {name}."),
            json!({ "provider": name, "source": null, "key": null }),
        );
    }
    let key = match lexi_host::config::resolve_api_key(name, &cfg) {
        Ok(k) => k,
        Err(e) => return io.fail(e),
    };
    let shown = if reveal { key } else { mask(&key) };
    io.result(
        &shown,
        json!({ "provider": name, "source": source, "key": shown }),
    )
}

fn key_remove(io: &mut Io<'_>, name: &str) -> u8 {
    if let Err(code) = provider_cfg(io, name) {
        return code;
    }
    match lexi_host::config::remove_api_key(name) {
        Ok(true) => io.result(
            format!("Removed API key for {name}."),
            json!({ "removed": true }),
        ),
        Ok(false) => io.result(
            format!("No API key file found for {name}."),
            json!({ "removed": false }),
        ),
        Err(e) => io.fail(e),
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
