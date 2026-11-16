//! `embed` — texts in, vectors out, via the provider's embedding binding.

use std::io::Read;

use serde_json::{Map, Value};

use crate::coerce::{coerce_typed, parse_set_arg};
use crate::output::{scalar, Io};

#[derive(Default)]
struct Plan {
    inputs: Vec<String>,
    model: Option<String>,
    set_paths: Vec<(String, Value)>,
    unset_paths: Vec<String>,
    no_validate: bool,
}

fn normalize(tok: &str) -> String {
    match tok.strip_prefix("--") {
        Some(rest) => format!("--{}", rest.replace('_', "-")),
        None => tok.to_string(),
    }
}

fn parse(args: &[String], flag_specs: &[Value]) -> Result<Plan, String> {
    let mut flags: std::collections::HashMap<String, &Value> = Default::default();
    for spec in flag_specs {
        for key in ["flag", "alias"] {
            if let Some(f) = spec.get(key).and_then(Value::as_str) {
                flags.insert(normalize(f), spec);
            }
        }
    }
    let mut plan = Plan::default();
    let mut i = 0;
    let value = |i: usize, flag: &str| -> Result<String, String> {
        args.get(i + 1)
            .cloned()
            .ok_or_else(|| format!("{flag} requires a value"))
    };
    while i < args.len() {
        let raw = &args[i];
        let tok = normalize(raw);
        let mut step = 2;
        match tok.as_str() {
            "--model" => plan.model = Some(value(i, "--model")?),
            "--file" => {
                let path = value(i, "--file")?;
                let text = std::fs::read_to_string(&path)
                    .map_err(|e| format!("cannot read --file {path:?}: {e}"))?;
                plan.inputs.extend(lines(&text));
            }
            "--set" => plan.set_paths.push(parse_set_arg(&value(i, "--set")?)?),
            "--unset" => plan.unset_paths.push(value(i, "--unset")?),
            "--no-validate" => {
                plan.no_validate = true;
                step = 1;
            }
            "-" => {
                let mut text = String::new();
                std::io::stdin()
                    .read_to_string(&mut text)
                    .map_err(|e| format!("could not read stdin: {e}"))?;
                plan.inputs.extend(lines(&text));
                step = 1;
            }
            _ if !raw.starts_with('-') => {
                plan.inputs.push(raw.clone());
                step = 1;
            }
            _ => {
                let Some(spec) = flags.get(&tok) else {
                    return Err(format!("unknown option {raw:?}"));
                };
                let path = scalar(spec.get("path"));
                if spec.get("type").and_then(Value::as_str) == Some("boolean") {
                    plan.set_paths.push((path, Value::Bool(true)));
                    step = 1;
                } else {
                    let v =
                        coerce_typed(&value(i, &tok)?, spec).map_err(|e| format!("{tok}: {e}"))?;
                    plan.set_paths.push((path, v));
                }
            }
        }
        i += step;
    }
    Ok(plan)
}

/// Non-empty lines, one text each.
fn lines(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect()
}

fn preview(text: &str) -> String {
    let t: String = text.chars().take(40).collect();
    if text.chars().count() > 40 {
        format!("{t}…")
    } else {
        t
    }
}

pub(crate) async fn embed(io: &mut Io<'_>, provider: &str, args: &[String]) -> u8 {
    let mut args = args.to_vec();
    if let Some(pos) = args.iter().position(|a| a == "--json") {
        args.remove(pos);
        io.json = true;
    }
    // Flags come from the embedding API that serves the model.
    let model = args
        .iter()
        .position(|a| a == "--model")
        .and_then(|i| args.get(i + 1).cloned());
    let eff = match lexi_host::resolve_model(provider, model.as_deref(), Some("vector")) {
        Ok(r) => r.effective,
        Err(e) => return io.fail(e),
    };
    let plan = match parse(&args, &lexi_core::cli_flags(&eff)) {
        Ok(p) => p,
        Err(e) => return io.fail(e),
    };
    let req = lexi_host::EmbedRequest {
        inputs: plan.inputs.clone(),
        model: plan.model,
        overrides: Map::new(),
        set_paths: plan.set_paths,
        unset_paths: plan.unset_paths,
        skip_validation: plan.no_validate,
    };
    let out = match lexi_host::embed(provider, &req).await {
        Ok(v) => v,
        Err(e) => return io.fail(e),
    };
    let vectors = out["embeddings"].as_array().cloned().unwrap_or_default();
    let mut lines = vec![format!(
        "{} embedding{} × {} dimensions ({})",
        out["count"],
        if out["count"] == 1 { "" } else { "s" },
        out["dimensions"],
        scalar(out.get("model"))
    )];
    for (text, v) in plan.inputs.iter().zip(&vectors) {
        let head: Vec<String> = v
            .as_array()
            .into_iter()
            .flatten()
            .take(4)
            .map(|x| format!("{:.4}", x.as_f64().unwrap_or(0.0)))
            .collect();
        lines.push(format!(
            "  \"{}\" → [{}, …]",
            preview(text),
            head.join(", ")
        ));
    }
    lines.push("(the full vectors: --json)".into());
    io.result(lines.join("\n"), out)
}
