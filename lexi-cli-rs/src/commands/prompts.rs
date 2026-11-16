//! `prompts` — saved prompts (`prompts/*.json`; the active one is named `$$`).

use serde_json::{json, Value};

use crate::cli::PromptsCmd;
use crate::output::{pretty, scalar, table, Io};

use lexi_host::admin::{prompt_defaults, prompt_value as prompt_json};
use lexi_host::config::ACTIVE_PROMPT;

pub(crate) fn run(io: &mut Io<'_>, cmd: PromptsCmd) -> u8 {
    match cmd {
        PromptsCmd::List {
            name,
            raw,
            detailed,
            table,
        } => list(io, name, raw, detailed, table),
        PromptsCmd::Set { args } => set(io, &args),
        PromptsCmd::Rm { name } => rm(io, &name),
        PromptsCmd::Activate { name } => activate(io, &name),
        PromptsCmd::Text(args) => adhoc(io, &args),
    }
}

/// Apply prompt text and options to `base` (the Python CLI's
/// `parse_prompt_options`): bare words and `--prompt` values form the text;
/// `-m/--max_tokens`, `-t/--temperature`, `-r/--role`, `--set path=value`,
/// `--unset path` edit fields.
fn apply_prompt_options(
    tokens: &[String],
    mut base: Value,
    require_prompt: bool,
) -> Result<Value, String> {
    let mut text: Vec<&str> = Vec::new();
    let mut i = 0;
    let value = |i: usize, what: &str| -> Result<&String, String> {
        tokens
            .get(i + 1)
            .ok_or_else(|| format!("{what} requires a value"))
    };
    while i < tokens.len() {
        let tok = tokens[i].as_str();
        match tok {
            "--max_tokens" | "--max-tokens" | "-m" => {
                let v = value(i, "max_tokens")?;
                let n: i64 = v
                    .parse()
                    .map_err(|_| "max_tokens must be an integer".to_string())?;
                base["max_tokens"] = Value::from(n);
                i += 2;
            }
            "--temperature" | "-t" => {
                let v = value(i, "temperature")?;
                let t: f64 = v
                    .parse()
                    .map_err(|_| "temperature must be numeric".to_string())?;
                base["temperature"] = Value::from(t);
                i += 2;
            }
            "--role" | "-r" => {
                base["role"] = Value::from(value(i, "role")?.as_str());
                i += 2;
            }
            "--prompt" => {
                text.push(value(i, "prompt")?);
                i += 2;
            }
            "--set" => {
                let (path, v) = crate::coerce::parse_set_arg(value(i, "--set")?)?;
                lexi_core::path::set_path(&mut base, &path, v);
                i += 2;
            }
            "--unset" => {
                lexi_core::path::unset_path(&mut base, value(i, "--unset")?);
                i += 2;
            }
            _ => {
                text.push(tok);
                i += 1;
            }
        }
    }
    let text = text.join(" ").trim().replace("\\n", "\n");
    if !text.is_empty() {
        base["prompt"] = Value::from(text);
    }
    if require_prompt && scalar(base.get("prompt")).is_empty() {
        return Err("prompt text is required".into());
    }
    Ok(base)
}

fn set(io: &mut Io<'_>, args: &[String]) -> u8 {
    // A leading bare word names the prompt when anything follows it; on its
    // own it is the active prompt's text.
    let (name, rest) = match args.first() {
        Some(first) if !first.starts_with('-') && args.len() >= 2 => (first.as_str(), &args[1..]),
        _ => (ACTIVE_PROMPT, args),
    };
    let existing = match lexi_host::config::load_prompts() {
        Ok(p) => p.into_iter().find(|(n, _)| n == name).map(|(_, b)| b),
        Err(e) => return io.fail(e),
    };
    let mut base = prompt_defaults();
    if let Some(Value::Object(fields)) = existing {
        for (k, v) in fields {
            base[k.as_str()] = v;
        }
    }
    let body = match apply_prompt_options(rest, base, false) {
        Ok(b) => b,
        Err(e) => return io.fail(e),
    };
    if scalar(body.get("prompt")).is_empty() {
        return io.fail("prompts set requires prompt text (use --prompt or supply text)");
    }
    match lexi_host::config::save_prompt(name, &body) {
        Ok(()) => io.result(format!("Saved prompt '{name}'."), prompt_json(name, &body)),
        Err(e) => io.fail(e),
    }
}

fn rm(io: &mut Io<'_>, name: &str) -> u8 {
    match lexi_host::config::load_prompts() {
        Ok(p) if p.iter().any(|(n, _)| n == name) => {}
        Ok(_) => return io.fail(format_args!("prompt {name:?} not found")),
        Err(e) => return io.fail(e),
    }
    match lexi_host::config::delete_prompt(name) {
        Ok(()) => io.result(
            format!("Removed prompt '{name}'."),
            json!({ "removed": name }),
        ),
        Err(e) => io.fail(e),
    }
}

fn activate(io: &mut Io<'_>, name: &str) -> u8 {
    match lexi_host::config::set_active_prompt(name) {
        Ok(()) => io.result(
            format!("Activated prompt '{name}'."),
            json!({ "active": name }),
        ),
        Err(e) => io.fail(e),
    }
}

fn adhoc(io: &mut Io<'_>, args: &[String]) -> u8 {
    match apply_prompt_options(args, prompt_defaults(), true) {
        Ok(body) => io.result(pretty(&body), body),
        Err(e) => io.fail(e),
    }
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
