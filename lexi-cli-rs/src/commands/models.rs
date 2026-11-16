//! `models` — a provider's configured models, hosted models, default and aliases.

use serde_json::{json, Value};

use crate::cli::{ModelAliasCmd, ModelsCmd};
use crate::output::{pretty, scalar, table, Io};

pub(crate) async fn run(io: &mut Io<'_>, cmd: ModelsCmd) -> u8 {
    match cmd {
        ModelsCmd::List {
            provider,
            raw,
            aliases,
            table,
            details,
        } => list(io, &provider, raw, aliases, table, details),
        ModelsCmd::Hosted {
            provider,
            raw,
            table,
        } => hosted(io, &provider, raw, table).await,
        ModelsCmd::Default { provider, model } => default(io, &provider, model.as_deref()),
        ModelsCmd::Alias { cmd } => match cmd {
            ModelAliasCmd::List { provider, raw } => alias_list(io, &provider, raw),
        },
    }
}

/// Fail unless `provider` is in providers-config.yaml.
fn require_provider(io: &mut Io<'_>, provider: &str) -> Result<Value, u8> {
    let cfg = lexi_host::config::load_providers().map_err(|e| io.fail(e))?;
    cfg.get("providers")
        .and_then(|p| p.get(provider))
        .cloned()
        .ok_or_else(|| io.fail(lexi_host::HostError::UnknownProvider(provider.into())))
}

/// The model's metadata other than `name`/`description`, sorted by key.
fn extra_metadata(meta: &Value) -> Vec<(String, String)> {
    let mut extra: Vec<(String, String)> = meta
        .as_object()
        .map(|m| {
            m.iter()
                .filter(|(k, _)| *k != "name" && *k != "description")
                .map(|(k, v)| (k.clone(), scalar(Some(v))))
                .collect()
        })
        .unwrap_or_default();
    extra.sort();
    extra
}

fn list(
    io: &mut Io<'_>,
    provider: &str,
    raw: bool,
    with_aliases: bool,
    as_table: bool,
    details: bool,
) -> u8 {
    if let Err(code) = require_provider(io, provider) {
        return code;
    }
    let models = match lexi_host::config::load_models(provider) {
        Ok(m) => m,
        Err(e) => return io.fail(e),
    };
    let aliases = match lexi_host::config::model_aliases(provider) {
        Ok(a) => a,
        Err(e) => return io.fail(e),
    };
    let json = json!(models);

    if raw {
        let mut text =
            super::read_home_file(&format!("providers/{provider}/configured-models.yaml"))
                .unwrap_or_default();
        if with_aliases {
            let a = super::read_home_file(&format!("providers/{provider}/model-aliases.yaml"))
                .unwrap_or_default();
            text = format!("{}\n# aliases\n{a}", text.trim_end());
        }
        return io.result(text, json);
    }
    if models.is_empty() {
        return io.result(format!("No models configured for {provider}."), json);
    }
    let human = if as_table {
        let rows = models
            .iter()
            .map(|m| {
                let name = m.name.clone().unwrap_or_else(|| m.id.clone());
                let label = if m.default {
                    format!("{name} (default)")
                } else {
                    name
                };
                let mut aliases = m.aliases.clone();
                aliases.sort();
                let mut desc: Vec<String> = m.description.iter().cloned().collect();
                if details {
                    desc.extend(
                        extra_metadata(&m.metadata)
                            .into_iter()
                            .map(|(k, v)| format!("{k}: {v}")),
                    );
                }
                vec![label, aliases.join(", "), desc.join("\n")]
            })
            .collect();
        table(
            &format!("Models for {provider}"),
            &["Model Name", "Alias", "Description"],
            rows,
        )
    } else {
        let mut lines = vec![format!("Models for {provider}:")];
        for m in &models {
            let mut extra = Vec::new();
            if m.default {
                extra.push("default".to_string());
            }
            if !m.aliases.is_empty() {
                let mut a = m.aliases.clone();
                a.sort();
                extra.push(format!("alias: {}", a.join(", ")));
            }
            let suffix = if extra.is_empty() {
                String::new()
            } else {
                format!(" ({})", extra.join(" "))
            };
            lines.push(format!(
                "  {}: {} - {}{suffix}",
                m.id,
                m.name.clone().unwrap_or_default(),
                m.description.clone().unwrap_or_default(),
            ));
            if details {
                for (k, v) in extra_metadata(&m.metadata) {
                    lines.push(format!("    {k}: {v}"));
                }
            }
        }
        if !aliases.is_empty() {
            lines.push("Aliases:".into());
            for (alias, target) in &aliases {
                lines.push(format!("  {alias} -> {}", scalar(Some(target))));
            }
        }
        lines.join("\n")
    };
    io.result(human, json)
}

async fn hosted(io: &mut Io<'_>, provider: &str, raw: bool, as_table: bool) -> u8 {
    let pcfg = match require_provider(io, provider) {
        Ok(c) => c,
        Err(code) => return code,
    };
    let items = match lexi_host::fetch_hosted_models(provider).await {
        Ok(i) => i,
        Err(e) => return io.fail(e),
    };
    let json = Value::Array(items.clone());
    if raw {
        return io.result(pretty(&json), json);
    }
    if items.is_empty() {
        return io.result(format!("No hosted models returned for {provider}."), json);
    }
    // Columns: the provider's configured hosted_model_fields, else the first
    // item's keys, else just the value itself.
    let fields: Vec<String> = pcfg
        .get("hosted_model_fields")
        .and_then(Value::as_array)
        .filter(|a| !a.is_empty())
        .map(|a| a.iter().map(|v| scalar(Some(v))).collect())
        .or_else(|| items[0].as_object().map(|o| o.keys().cloned().collect()))
        .unwrap_or_else(|| vec!["id".into()]);
    let cells = |item: &Value| -> Vec<String> {
        match item.as_object() {
            Some(o) => fields.iter().map(|f| scalar(o.get(f))).collect(),
            None => {
                let mut row = vec![scalar(Some(item))];
                row.resize(fields.len(), String::new());
                row
            }
        }
    };
    let human = if as_table {
        let headers: Vec<&str> = fields.iter().map(String::as_str).collect();
        table(
            &format!("Hosted models for {provider}"),
            &headers,
            items.iter().map(cells).collect(),
        )
    } else {
        let mut lines = vec![format!("Hosted models for {provider}:")];
        for item in &items {
            if item.is_object() {
                let pairs: Vec<String> = fields
                    .iter()
                    .zip(cells(item))
                    .map(|(f, v)| format!("{f}={v}"))
                    .collect();
                lines.push(format!("  {}", pairs.join(", ")));
            } else {
                lines.push(format!("  {}", scalar(Some(item))));
            }
        }
        lines.join("\n")
    };
    io.result(human, json)
}

fn default(io: &mut Io<'_>, provider: &str, model: Option<&str>) -> u8 {
    if let Err(code) = require_provider(io, provider) {
        return code;
    }
    let Some(model) = model else {
        return match lexi_host::config::default_model(provider) {
            Ok(Some(m)) => io.result(
                format!("Default model for {provider}: {m}"),
                json!({ "provider": provider, "default_model": m }),
            ),
            Ok(None) => io.result(
                format!("No default model set for {provider}."),
                json!({ "provider": provider, "default_model": null }),
            ),
            Err(e) => io.fail(e),
        };
    };
    match lexi_host::config::load_models(provider) {
        Ok(models) if models.iter().any(|m| m.id == model) => {}
        Ok(_) => {
            return io.fail(format_args!(
                "model {model:?} not found for provider {provider:?}"
            ))
        }
        Err(e) => return io.fail(e),
    }
    match lexi_host::config::set_default_model(provider, Some(model)) {
        Ok(()) => io.result(
            format!("Set default model for {provider} to {model}."),
            json!({ "provider": provider, "default_model": model }),
        ),
        Err(e) => io.fail(e),
    }
}

fn alias_list(io: &mut Io<'_>, provider: &str, raw: bool) -> u8 {
    if let Err(code) = require_provider(io, provider) {
        return code;
    }
    let aliases = match lexi_host::config::model_aliases(provider) {
        Ok(a) => a,
        Err(e) => return io.fail(e),
    };
    let json = Value::Array(
        aliases
            .iter()
            .map(|(a, m)| json!({ "alias": a, "model": m }))
            .collect(),
    );
    if raw {
        let path = format!("providers/{provider}/model-aliases.yaml");
        let text = super::read_home_file(&path)
            .unwrap_or_else(|_| format!("No aliases file for {provider}."));
        return io.result(text, json);
    }
    let human = if aliases.is_empty() {
        format!("No aliases configured for {provider}.")
    } else {
        aliases
            .iter()
            .map(|(a, m)| format!("  {a} -> {}", scalar(Some(m))))
            .collect::<Vec<_>>()
            .join("\n")
    };
    io.result(human, json)
}
