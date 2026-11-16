//! `api` — API definitions (shipped ones embedded in the binary, custom ones in
//! `<home>/apis/`).

use serde_json::{json, Map, Value};

use crate::cli::ApiCmd;
use crate::output::{pretty, scalar, table, Io, EXIT_FAILURE};

pub(crate) fn run(io: &mut Io<'_>, cmd: ApiCmd) -> u8 {
    match cmd {
        ApiCmd::List { raw } => list(io, raw),
        ApiCmd::Show { name, raw } => show(io, &name, raw),
        ApiCmd::Validate { name } => validate(io, &name),
        ApiCmd::Create {
            name,
            from,
            path,
            text_path,
        } => create(
            io,
            &name,
            from.as_deref(),
            path.as_deref(),
            text_path.as_deref(),
        ),
        ApiCmd::Edit {
            name,
            path,
            text_path,
            set_meta,
        } => edit(io, &name, path.as_deref(), text_path.as_deref(), &set_meta),
        ApiCmd::Preview {
            name,
            file,
            prompt,
            model,
            role,
        } => preview(io, name.as_deref(), file.as_deref(), &prompt, &model, &role),
        ApiCmd::Save { name, file } => save(io, &name, &file),
        ApiCmd::Rm { name } => match lexi_host::config::delete_api_def(&name) {
            Ok(()) => io.result(
                format!("API definition '{name}' removed."),
                json!({ "removed": name }),
            ),
            Err(e) => io.fail(e),
        },
    }
}

/// A definition's response text path, as the Python CLI's list shows it.
fn text_path(def: &Value) -> String {
    let te = def.pointer("/response/text_extract");
    if let Some(p) = te.and_then(|t| t.get("text_path")).and_then(Value::as_str) {
        return p.to_string();
    }
    match te.and_then(|t| t.get("items_path")).and_then(Value::as_str) {
        Some(items) => format!("items:{items}"),
        None => String::new(),
    }
}

fn list(io: &mut Io<'_>, raw: bool) -> u8 {
    let defs = match lexi_host::config::list_api_defs() {
        Ok(d) => d,
        Err(e) => return io.fail(e),
    };
    let shipped = lexi_host::config::embedded_api_names();
    let mut all = Map::new();
    let mut rows = Vec::new();
    for (name, desc) in &defs {
        let def = match lexi_host::config::load_api_def(name) {
            Ok(d) => d,
            Err(e) => return io.fail(e),
        };
        rows.push(json!({
            "name": name,
            "description": desc,
            "path": def.pointer("/request/path"),
            "text_path": text_path(&def),
            "builtin": shipped.contains(&name.as_str()),
        }));
        all.insert(name.clone(), def);
    }
    let human = if raw {
        pretty(&Value::Object(all))
    } else if rows.is_empty() {
        "No API definitions found.".to_string()
    } else {
        table(
            "API Definitions",
            &["Name", "Description", "Path", "Text Path"],
            rows.iter()
                .map(|r| {
                    vec![
                        scalar(r.get("name")),
                        scalar(r.get("description")),
                        scalar(r.get("path")),
                        scalar(r.get("text_path")),
                    ]
                })
                .collect(),
        )
    };
    io.result(human, Value::Array(rows))
}

fn show(io: &mut Io<'_>, name: &str, raw: bool) -> u8 {
    let def = match lexi_host::config::load_api_def(name) {
        Ok(d) => d,
        Err(e) => return io.fail(e),
    };
    let human = if raw {
        pretty(&def)
    } else {
        summary(name, &def)
    };
    io.result(human, def)
}

/// The human summary of a definition (mirrors the Python CLI's `api show`).
fn summary(name: &str, def: &Value) -> String {
    let mut out = vec![name.to_string()];
    let desc = scalar(def.get("description"));
    if !desc.is_empty() {
        out.push(format!("  {desc}"));
    }
    if let Some(auth) = def.get("auth").and_then(Value::as_object) {
        let header = auth
            .get("header")
            .and_then(Value::as_str)
            .unwrap_or("Authorization");
        let prefix = auth
            .get("value_prefix")
            .and_then(Value::as_str)
            .unwrap_or("Bearer ");
        out.push(format!("  Auth: header={header}, prefix={prefix:?}"));
        if let Some(extra) = auth.get("extra_headers").and_then(Value::as_object) {
            for (k, v) in extra {
                out.push(format!("    {k}: {}", scalar(Some(v))));
            }
        }
    }
    if let Some(req) = def.get("request").and_then(Value::as_object) {
        let method = req.get("method").and_then(Value::as_str).unwrap_or("POST");
        out.push(format!("  Request: {method} {}", scalar(req.get("path"))));
        let schema = req.get("body_schema");
        let required: Vec<&str> = schema
            .and_then(|s| s.get("required"))
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        if let Some(props) = schema
            .and_then(|s| s.get("properties"))
            .and_then(Value::as_object)
            .filter(|p| !p.is_empty())
        {
            out.push("  Body schema properties:".into());
            for (pname, pdef) in props {
                if pdef.is_object() {
                    out.push(property_line(
                        pname,
                        pdef,
                        required.contains(&pname.as_str()),
                    ));
                }
            }
        }
    }
    if let Some(presets) = def
        .get("tool_presets")
        .and_then(Value::as_object)
        .filter(|p| !p.is_empty())
    {
        let mut names: Vec<&str> = presets.keys().map(String::as_str).collect();
        names.sort();
        out.push(format!("  Tool presets: {}", names.join(", ")));
    }
    if let Some(resp) = def.get("response").and_then(Value::as_object) {
        let te = resp.get("text_extract");
        let text = if let Some(p) = te.and_then(|t| t.get("text_path")) {
            format!("text_path={}", scalar(Some(p)))
        } else if let Some(items) = te.and_then(|t| t.get("items_path")) {
            let content = te
                .and_then(|t| t.get("content_path"))
                .map(|c| scalar(Some(c)))
                .unwrap_or_else(|| "(item)".into());
            format!(
                "items_path={} item_filter={} content_path={content}",
                scalar(Some(items)),
                te.and_then(|t| t.get("item_filter"))
                    .cloned()
                    .unwrap_or_else(|| json!({}))
            )
        } else if resp.contains_key("artifacts") {
            "artifacts".to_string()
        } else {
            "(no text_extract configured)".to_string()
        };
        out.push(format!("  Response: {text}"));
    }
    out.join("\n")
}

fn property_line(name: &str, def: &Value, required: bool) -> String {
    let ty = match def.get("type") {
        Some(Value::Array(types)) => types
            .iter()
            .map(|t| scalar(Some(t)))
            .collect::<Vec<_>>()
            .join("|"),
        other => scalar(other),
    };
    let mut line = format!("    {name} ({ty})");
    if required {
        line.push_str(" [required]");
    }
    if let Some(e) = def.get("enum") {
        line.push_str(&format!(" enum={e}"));
    } else if def.get("minimum").is_some() || def.get("maximum").is_some() {
        let bound = |k: &str| def.get(k).map(|v| scalar(Some(v))).unwrap_or("-".into());
        line.push_str(&format!(
            " range=[{}, {}]",
            bound("minimum"),
            bound("maximum")
        ));
    }
    if let Some(d) = def.get("default") {
        line.push_str(&format!(" default={}", scalar(Some(d))));
    }
    if let Some(flag) = def.pointer("/x-cli/flag").and_then(Value::as_str) {
        line.push_str(&format!(" cli={flag}"));
    }
    if let Some(desc) = def.get("description").and_then(Value::as_str) {
        line.push_str(&format!(" — {desc}"));
    }
    line
}

fn validate(io: &mut Io<'_>, name: &str) -> u8 {
    let (valid, errors) = match lexi_host::config::validate_api_def(name) {
        Ok(r) => r,
        Err(e) => return io.fail(e),
    };
    let json = json!({ "name": name, "valid": valid, "errors": errors });
    if valid {
        return io.result(format!("API definition '{name}' is valid."), json);
    }
    if io.json {
        io.json_line(&json);
    } else {
        let mut msg = format!("Validation errors for '{name}':");
        for e in &errors {
            msg.push_str(&format!("\n  - {e}"));
        }
        io.fail(msg);
    }
    EXIT_FAILURE
}

fn create(
    io: &mut Io<'_>,
    name: &str,
    from: Option<&str>,
    path: Option<&str>,
    text_path: Option<&str>,
) -> u8 {
    if lexi_host::config::load_api_def(name).is_ok() {
        return io.fail(format_args!(
            "API definition {name:?} already exists; use `api edit` to modify it"
        ));
    }
    match lexi_host::config::create_api_def(name, from, path, text_path) {
        Ok(()) => io.result(
            format!("API definition '{name}' created."),
            json!({ "created": name }),
        ),
        Err(e) => io.fail(e),
    }
}

fn edit(
    io: &mut Io<'_>,
    name: &str,
    path: Option<&str>,
    text_path: Option<&str>,
    set_meta: &[String],
) -> u8 {
    let mut meta = Vec::new();
    for m in set_meta {
        match m.split_once(':') {
            Some((k, v)) if !k.is_empty() => meta.push((k.to_string(), v.to_string())),
            _ => return io.fail(format_args!("--set-meta format is KEY:PATH (got {m:?})")),
        }
    }
    match lexi_host::config::edit_api_def(name, path, text_path, &meta) {
        Ok(()) => io.result(
            format!("API definition '{name}' updated."),
            json!({ "updated": name }),
        ),
        Err(e) => io.fail(e),
    }
}

fn preview(
    io: &mut Io<'_>,
    name: Option<&str>,
    file: Option<&std::path::Path>,
    prompt: &str,
    model: &str,
    role: &str,
) -> u8 {
    let yaml = match file.map(std::fs::read_to_string).transpose() {
        Ok(y) => y,
        Err(e) => return io.fail(format_args!("could not read file: {e}")),
    };
    match lexi_host::config::preview_api_def(name, yaml.as_deref(), prompt, model, role) {
        Ok(result) => io.result(pretty(&result), result),
        Err(e) => io.fail(e),
    }
}

fn save(io: &mut Io<'_>, name: &str, file: &std::path::Path) -> u8 {
    if lexi_host::config::embedded_api_names().contains(&name) {
        return io.fail(format_args!(
            "{name:?} is a shipped definition and can't be replaced; save under another name"
        ));
    }
    let yaml = match std::fs::read_to_string(file) {
        Ok(y) => y,
        Err(e) => return io.fail(format_args!("could not read file: {e}")),
    };
    match lexi_host::config::save_api_raw(name, &yaml) {
        Ok(errors) if errors.is_empty() => io.result(
            format!("API definition '{name}' saved."),
            json!({ "saved": name, "valid": true, "errors": [] }),
        ),
        Ok(errors) => {
            let mut human = format!("API definition '{name}' saved, with validation errors:");
            for e in &errors {
                human.push_str(&format!("\n  - {e}"));
            }
            io.result(
                human,
                json!({ "saved": name, "valid": false, "errors": errors }),
            )
        }
        Err(e) => io.fail(e),
    }
}
