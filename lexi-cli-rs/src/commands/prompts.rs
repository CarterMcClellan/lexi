//! `prompts` — saved prompts (`prompts/*.json`; the active one is named `$$`).

use serde_json::{json, Map, Value};

use crate::cli::PromptsCmd;
use crate::output::{pretty, scalar, table, Io};

/// The saved-prompt name that holds the active prompt.
pub(crate) const ACTIVE_PROMPT: &str = "$$";

pub(crate) fn run(io: &mut Io<'_>, cmd: PromptsCmd) -> u8 {
    match cmd {
        PromptsCmd::List {
            name,
            raw,
            detailed,
            table,
        } => list(io, name, raw, detailed, table),
    }
}

/// A prompt as JSON output: its body plus `name` and `active`.
fn prompt_json(name: &str, body: &Value) -> Value {
    let mut obj = Map::new();
    obj.insert("name".into(), Value::from(name));
    obj.insert("active".into(), Value::from(name == ACTIVE_PROMPT));
    if let Value::Object(fields) = body {
        for (k, v) in fields {
            obj.entry(k.clone()).or_insert_with(|| v.clone());
        }
    }
    Value::Object(obj)
}

fn line(name: &str, body: &Value, detailed: bool) -> String {
    let marker = if name == ACTIVE_PROMPT { "* " } else { "" };
    let mut s = format!("{marker}{name}: {}", scalar(body.get("prompt")));
    if detailed {
        s.push_str(&format!(
            "\n    max_tokens={} temperature={} role={}",
            scalar(body.get("max_tokens")),
            scalar(body.get("temperature")),
            scalar(body.get("role")),
        ));
    }
    s
}

fn table_rows(prompts: &[(String, Value)]) -> Vec<Vec<String>> {
    prompts
        .iter()
        .map(|(name, body)| {
            vec![
                name.clone(),
                scalar(body.get("prompt")),
                scalar(body.get("max_tokens")),
                scalar(body.get("temperature")),
                scalar(body.get("role")),
            ]
        })
        .collect()
}

const HEADERS: &[&str] = &["Name", "Prompt", "max_tokens", "temperature", "role"];

fn list(io: &mut Io<'_>, name: Option<String>, raw: bool, detailed: bool, as_table: bool) -> u8 {
    let mut prompts = match lexi_host::config::load_prompts() {
        Ok(p) => p,
        Err(e) => return io.fail(e),
    };
    // Active prompt first, then the rest by name.
    prompts.sort_by_key(|(n, _)| (n != ACTIVE_PROMPT, n.clone()));

    if let Some(name) = name {
        let Some((_, body)) = prompts.iter().find(|(n, _)| *n == name) else {
            return io.fail(format_args!("prompt {name:?} not found"));
        };
        let one = [(name.clone(), body.clone())];
        let human = if raw {
            pretty(&json!({ name.clone(): body }))
        } else if as_table {
            table(&format!("Prompt {name}"), HEADERS, table_rows(&one))
        } else {
            line(&name, body, detailed)
        };
        return io.result(human, prompt_json(&name, body));
    }

    let json = Value::Array(prompts.iter().map(|(n, b)| prompt_json(n, b)).collect());
    let human = if prompts.is_empty() {
        "No prompts saved.".to_string()
    } else if raw {
        pretty(&Value::Object(prompts.iter().cloned().collect()))
    } else if as_table {
        table("Prompts", HEADERS, table_rows(&prompts))
    } else {
        prompts
            .iter()
            .map(|(n, b)| line(n, b, detailed))
            .collect::<Vec<_>>()
            .join("\n")
    };
    io.result(human, json)
}
