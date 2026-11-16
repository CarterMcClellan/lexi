//! `respond` — send a saved prompt (or inline text) to a provider.
//!
//! Grammar (the Python CLI's, plus the Rust host's media/stream options):
//!
//! ```text
//! respond <provider> [prompt_name] [--text TEXT|-] [--model M] [--role R]
//!         [--stream] [--background] [--image URL|PATH]…
//!         [<schema flags>] [--set path=value]… [--unset path]…
//!         [--tool preset[,preset]]… [--params FILE] [--cite]
//!         [--raw|-r] [--verbose|-v] [--no-validate]
//! ```
//!
//! Schema flags come from the provider's API definition (`x-cli` in its
//! `body_schema`; see `lexi api show <api>`), so they are parsed here rather
//! than declared to clap.

use std::io::Read;

use serde_json::{json, Map, Value};

use crate::coerce::{coerce_typed, parse_set_arg};
use crate::commands::prompts::ACTIVE_PROMPT;
use crate::output::{pretty, Io, EXIT_FAILURE, EXIT_OK};

#[derive(Debug, Default)]
struct Plan {
    prompt_name: Option<String>,
    text: Option<String>,
    role: Option<String>,
    model: Option<String>,
    stream: bool,
    background: bool,
    detach: bool,
    images: Vec<String>,
    set_paths: Vec<(String, Value)>,
    unset_paths: Vec<String>,
    tools: Vec<String>,
    params_file: Option<String>,
    cite: bool,
    raw: bool,
    verbose: bool,
    no_validate: bool,
}

/// `--max_tokens` and `--max-tokens` are the same flag.
fn normalize(tok: &str) -> String {
    match tok.strip_prefix("--") {
        Some(rest) => format!("--{}", rest.replace('_', "-")),
        None => tok.to_string(),
    }
}

fn parse(args: &[String], flag_specs: &[Value], presets: &[String]) -> Result<Plan, String> {
    let mut flag_map: std::collections::HashMap<String, &Value> = Default::default();
    for spec in flag_specs {
        for key in ["flag", "alias"] {
            if let Some(f) = spec.get(key).and_then(Value::as_str) {
                flag_map.insert(normalize(f), spec);
            }
        }
    }
    let mut plan = Plan::default();
    let mut i = 0;
    if let Some(first) = args.first() {
        if !first.starts_with('-') {
            plan.prompt_name = Some(first.clone());
            i = 1;
        }
    }
    let value = |i: usize, flag: &str| -> Result<String, String> {
        args.get(i + 1)
            .cloned()
            .ok_or_else(|| format!("{flag} requires a value"))
    };
    while i < args.len() {
        let raw_tok = &args[i];
        let tok = normalize(raw_tok);
        let mut step = 2;
        match tok.as_str() {
            "--text" => plan.text = Some(value(i, "--text")?),
            "--role" => plan.role = Some(value(i, "--role")?),
            "--model" => plan.model = Some(value(i, "--model")?),
            "--image" => plan.images.push(value(i, "--image")?),
            "--params" => plan.params_file = Some(value(i, "--params")?),
            "--set" => plan.set_paths.push(parse_set_arg(&value(i, "--set")?)?),
            "--unset" => plan.unset_paths.push(value(i, "--unset")?),
            "--tool" => {
                for name in value(i, "--tool")?.split(',').map(str::trim) {
                    if name.is_empty() {
                        continue;
                    }
                    if !presets.iter().any(|p| p == name) {
                        return Err(format!(
                            "--tool: unknown preset {name:?} (available: {})",
                            presets.join(", ")
                        ));
                    }
                    plan.tools.push(name.to_string());
                }
            }
            "--stream" | "--background" | "--detach" | "--cite" | "--raw" | "-r" | "--verbose"
            | "-v" | "--no-validate" => {
                step = 1;
                match tok.as_str() {
                    "--stream" => plan.stream = true,
                    "--background" => plan.background = true,
                    "--detach" => plan.detach = true,
                    "--cite" => plan.cite = true,
                    "--raw" | "-r" => plan.raw = true,
                    "--verbose" | "-v" => plan.verbose = true,
                    _ => plan.no_validate = true,
                }
            }
            _ => {
                let Some(spec) = flag_map.get(&tok) else {
                    return Err(format!("unknown option {raw_tok:?}"));
                };
                let path = spec
                    .get("path")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                if spec.get("type").and_then(Value::as_str) == Some("boolean") {
                    let invert = spec.get("invert").and_then(Value::as_bool) == Some(true);
                    plan.set_paths.push((path, Value::Bool(!invert)));
                    step = 1;
                } else if spec.get("repeat").and_then(Value::as_bool) == Some(true) {
                    let v = Value::from(value(i, &tok)?);
                    match plan.set_paths.iter_mut().find(|(p, _)| *p == path) {
                        Some((_, Value::Array(items))) => items.push(v),
                        _ => plan.set_paths.push((path, Value::Array(vec![v]))),
                    }
                } else {
                    let v =
                        coerce_typed(&value(i, &tok)?, spec).map_err(|e| format!("{tok}: {e}"))?;
                    plan.set_paths.push((path, v));
                }
            }
        }
        i += step;
    }
    if plan.text.is_some() && plan.prompt_name.is_some() {
        return Err("give a prompt name or --text, not both".into());
    }
    if plan.stream && plan.background {
        return Err("--stream and --background can't be combined".into());
    }
    if plan.detach && !plan.background {
        return Err("--detach needs --background".into());
    }
    Ok(plan)
}

/// The prompt payload: `--text` (or stdin for `-`), else the named or active
/// saved prompt; `--role` overrides its role.
fn prompt_payload(plan: &Plan) -> Result<Value, String> {
    let mut payload = match &plan.text {
        Some(t) if t == "-" => {
            let mut text = String::new();
            std::io::stdin()
                .read_to_string(&mut text)
                .map_err(|e| format!("could not read stdin: {e}"))?;
            json!({ "prompt": text.trim_end(), "role": "user" })
        }
        Some(t) => json!({ "prompt": t, "role": "user" }),
        None => {
            let name = plan.prompt_name.as_deref().unwrap_or(ACTIVE_PROMPT);
            let prompts = lexi_host::config::load_prompts().map_err(|e| e.to_string())?;
            match prompts.into_iter().find(|(n, _)| n == name) {
                Some((_, body)) => body,
                None if name == ACTIVE_PROMPT => {
                    return Err("no active prompt; save one with `prompts set <text>`, \
                                name a saved prompt, or pass --text"
                        .into())
                }
                None => return Err(format!("prompt {name:?} not found")),
            }
        }
    };
    if let Some(role) = &plan.role {
        payload["role"] = Value::from(role.as_str());
    }
    Ok(payload)
}

fn load_params(path: &str) -> Result<Map<String, Value>, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read --params file {path:?}: {e}"))?;
    let value: Value = if path.ends_with(".json") {
        serde_json::from_str(&text)
            .map_err(|e| format!("--params: invalid JSON in {path:?}: {e}"))?
    } else {
        serde_yaml::from_str(&text)
            .map_err(|e| format!("--params: invalid YAML in {path:?}: {e}"))?
    };
    match value {
        Value::Object(m) => Ok(m),
        _ => Err(format!("--params: {path:?} must contain a mapping")),
    }
}

/// `response.metadata` (key → path) resolved against the raw response.
fn metadata(raw: &Value, eff: &Value) -> Map<String, Value> {
    let mut out = Map::new();
    if let Some(paths) = eff.pointer("/response/metadata").and_then(Value::as_object) {
        for (k, p) in paths {
            if let Some(v) = p.as_str().and_then(|p| lexi_core::get_path(raw, p)) {
                if !v.is_null() {
                    out.insert(k.clone(), v.clone());
                }
            }
        }
    }
    out
}

/// The `[model | N in + M out = T tokens | stop]` and `[activity]` lines.
fn metadata_lines(meta: &Map<String, Value>, activity: &str) -> Vec<String> {
    let s = |k: &str| meta.get(k).map(|v| crate::output::scalar(Some(v)));
    let mut parts = Vec::new();
    if let Some(m) = s("model") {
        parts.push(m);
    }
    match (s("input_tokens"), s("output_tokens"), s("total_tokens")) {
        (Some(i), Some(o), total) => parts.push(match total {
            Some(t) => format!("{i} in + {o} out = {t} tokens"),
            None => format!("{i} in + {o} out"),
        }),
        (_, _, Some(t)) => parts.push(format!("{t} tokens")),
        _ => {}
    }
    if let Some(stop) = s("stop_reason") {
        parts.push(stop);
    }
    let mut lines = Vec::new();
    if !parts.is_empty() {
        lines.push(format!("[{}]", parts.join(" | ")));
    }
    if !activity.is_empty() {
        lines.push(format!("[{activity}]"));
    }
    lines
}

/// Rendered citations (`response.annotations`), one per line.
fn sources(raw: &Value, eff: &Value) -> Vec<String> {
    let resp = eff.get("response");
    let Some(ann_cfg) = resp.and_then(|r| r.get("annotations")) else {
        return Vec::new();
    };
    let te = resp.and_then(|r| r.get("text_extract"));
    let items = lexi_core::find_text_items(raw, te);
    let anns = lexi_core::extract_annotations(raw, Some(ann_cfg), &items);
    let rendered = lexi_core::render_annotations(&anns, Some(ann_cfg));
    rendered
        .lines()
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
}

pub(crate) async fn respond(io: &mut Io<'_>, provider: &str, args: &[String]) -> u8 {
    // `--json` is global, but after the provider it lands in this free-form
    // argument list; honor it wherever it appears.
    let mut args: Vec<String> = args.to_vec();
    if let Some(pos) = args.iter().position(|a| a == "--json") {
        args.remove(pos);
        io.json = true;
    }
    let args = &args[..];
    // The schema (for flags) comes from the provider's effective config.
    let flag_cfg = lexi_host::config::load_providers().and_then(|all| {
        let cfg = all
            .get("providers")
            .and_then(|p| p.get(provider))
            .cloned()
            .ok_or_else(|| lexi_host::HostError::UnknownProvider(provider.into()))?;
        lexi_host::config::effective_config(provider, &cfg)
    });
    let flag_cfg = match flag_cfg {
        Ok(c) => c,
        Err(e) => return io.fail(e),
    };
    let flags = lexi_core::cli_flags(&flag_cfg);
    let presets = lexi_core::tool_preset_names(&flag_cfg);
    let plan = match parse(args, &flags, &presets) {
        Ok(p) => p,
        Err(e) => return io.fail(e),
    };

    let payload = match prompt_payload(&plan) {
        Ok(p) => p,
        Err(e) => return io.fail(e),
    };
    let overrides = match plan.params_file.as_deref().map(load_params).transpose() {
        Ok(o) => o.unwrap_or_default(),
        Err(e) => return io.fail(e),
    };
    let images = match lexi_host::resolve_images(&plan.images) {
        Ok(i) => i,
        Err(e) => return io.fail(e),
    };
    let req = lexi_host::ChatRequest {
        prompt: payload,
        model: plan.model.clone(),
        images,
        overrides,
        tool_presets: plan.tools.clone(),
        set_paths: plan.set_paths.clone(),
        unset_paths: plan.unset_paths.clone(),
        skip_validation: plan.no_validate,
    };
    let mode = if plan.stream {
        lexi_host::ChatMode::Stream
    } else if plan.background {
        lexi_host::ChatMode::Background
    } else {
        lexi_host::ChatMode::Buffered
    };
    let chat = match lexi_host::prepare_chat(provider, &req, mode) {
        Ok(c) => c,
        Err(e) => return io.fail(e),
    };

    if plan.detach {
        return match lexi_host::submit_chat(&chat).await {
            Ok(st) => {
                let id = crate::output::scalar(st.operation.get("id"));
                let state = crate::output::scalar(st.operation.get("state"));
                let human = format!(
                    "Submitted job {id} ({state}).\nCheck it with: lexi jobs status {provider} {id}\n\
                     Wait for it with: lexi jobs wait {provider} {id}"
                );
                let mut json = st.operation.clone();
                json["provider"] = Value::from(provider);
                io.result(human, json)
            }
            Err(e) => io.fail(e),
        };
    }
    let streaming = chat.streams();
    let json_mode = io.json;
    let show_text = !plan.raw;
    let color = io.color;
    if streaming && json_mode {
        io.json_line(&json!({ "event": "provider_start", "provider": provider }));
    }
    let mut printed_delta = false;
    let result = {
        let (out, err) = (&mut *io.out, &mut *io.err);
        lexi_host::run_chat(&chat, |ev| match ev {
            lexi_host::StreamEvent::Delta(text) => {
                if json_mode {
                    let ev = json!({ "event": "delta", "provider": provider, "text": text });
                    let _ = writeln!(out, "{ev}");
                } else if show_text {
                    let _ = write!(out, "{text}");
                    printed_delta = true;
                }
                let _ = out.flush();
            }
            lexi_host::StreamEvent::Activity("") => {}
            lexi_host::StreamEvent::Activity(label) => {
                if json_mode {
                    let ev = json!({ "event": "activity", "provider": provider, "message": label });
                    let _ = writeln!(out, "{ev}");
                    let _ = out.flush();
                } else {
                    let _ = writeln!(err, "{}", crate::output::dim(label, color));
                    let _ = err.flush();
                }
            }
        })
        .await
    };
    if printed_delta {
        let _ = writeln!(io.out);
    }
    let res = match result {
        Ok(r) => r,
        Err(e) if streaming && json_mode => {
            io.json_line(&json!({
                "event": "provider_done",
                "provider": provider,
                "error": e.to_string(),
            }));
            return EXIT_FAILURE;
        }
        Err(e) => return io.fail(e),
    };

    let meta = metadata(&res.raw, &chat.effective);
    let activity_cfg = chat.effective.pointer("/response/activity");
    let activity = lexi_core::summarize_activity(&res.raw, activity_cfg);
    let cites = if plan.cite {
        sources(&res.raw, &chat.effective)
    } else {
        Vec::new()
    };

    if json_mode {
        let mut done = json!({
            "provider": provider,
            "model": chat.model,
            "text": res.text,
            "metadata": meta,
        });
        if !activity.is_empty() {
            done["activity"] = Value::from(activity);
        }
        if plan.cite {
            done["sources"] = json!(cites);
        }
        if plan.raw {
            done["raw"] = res.raw.clone();
        }
        if streaming {
            done["event"] = Value::from("provider_done");
            io.json_line(&done);
        } else {
            io.json_line(&done);
        }
        return EXIT_OK;
    }

    if plan.raw {
        if res.raw.is_null() {
            return io.fail("no response payload (the stream sent no final response event)");
        }
        let _ = writeln!(io.out, "{}", pretty(&res.raw));
        return EXIT_OK;
    }
    if !streaming {
        let text = if res.text.is_empty() {
            pretty(&res.raw)
        } else {
            res.text.clone()
        };
        let _ = writeln!(io.out, "{text}");
    }
    if !cites.is_empty() {
        let _ = writeln!(io.out, "\nSources:");
        for line in &cites {
            let _ = writeln!(io.out, "  {line}");
        }
    }
    if plan.verbose {
        for line in metadata_lines(&meta, &activity) {
            let _ = writeln!(io.err, "{}", crate::output::dim(&line, color));
        }
    }
    let _ = io.out.flush();
    let _ = io.err.flush();
    EXIT_OK
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &[&str]) -> Vec<String> {
        s.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn parses_builtins_schema_flags_and_presets() {
        let flags = vec![
            json!({"flag": "--max-tokens", "alias": "-m", "path": "max_tokens", "type": "integer"}),
            json!({"flag": "--stop", "path": "stop", "type": "array", "repeat": true}),
            json!({"flag": "--logprobs", "path": "logprobs", "type": "boolean"}),
        ];
        let presets = vec!["web_search".to_string()];
        let plan = parse(
            &args(&[
                "greet",
                "--max_tokens",
                "9",
                "--stop",
                "a",
                "--stop",
                "b",
                "--logprobs",
                "--tool",
                "web_search",
                "--set",
                "x.y=1",
                "--unset",
                "z",
                "-r",
                "--cite",
            ]),
            &flags,
            &presets,
        )
        .unwrap();
        assert_eq!(plan.prompt_name.as_deref(), Some("greet"));
        assert_eq!(
            plan.set_paths,
            vec![
                ("max_tokens".into(), json!(9)),
                ("stop".into(), json!(["a", "b"])),
                ("logprobs".into(), json!(true)),
                ("x.y".into(), json!(1)),
            ]
        );
        assert_eq!(plan.unset_paths, vec!["z".to_string()]);
        assert_eq!(plan.tools, vec!["web_search".to_string()]);
        assert!(plan.raw && plan.cite);

        assert!(parse(&args(&["--nope"]), &flags, &presets)
            .unwrap_err()
            .contains("unknown option"));
        assert!(parse(&args(&["--tool", "x"]), &flags, &presets)
            .unwrap_err()
            .contains("unknown preset"));
        assert!(parse(&args(&["--max-tokens", "lots"]), &flags, &presets)
            .unwrap_err()
            .contains("integer"));
        assert!(parse(&args(&["n", "--text", "t"]), &flags, &presets).is_err());
        assert!(parse(&args(&["--stream", "--background"]), &flags, &presets).is_err());
    }

    #[test]
    fn formats_metadata_lines() {
        let meta: Map<String, Value> = serde_json::from_value(json!({
            "model": "m", "input_tokens": 1, "output_tokens": 2, "total_tokens": 3, "stop_reason": "end"
        }))
        .unwrap();
        assert_eq!(
            metadata_lines(&meta, "1 web search"),
            vec!["[m | 1 in + 2 out = 3 tokens | end]", "[1 web search]"]
        );
        assert!(metadata_lines(&Map::new(), "").is_empty());
    }
}
