//! Structural validation of a schema-driven API definition.
//!
//! A language-neutral port of the CLI's `validate_api_definition`: given a parsed
//! definition, return a list of human-readable error messages (empty = valid).
//! Pure — no I/O — so the same checks run in the engine, the wheel, and the GUI.

use serde_json::Value;

/// The valid `x-include` modes a `body_schema` property may declare.
const VALID_INCLUDES: &[&str] = &[
    "always",
    "if_set",
    "if_truthy",
    "if_non_empty",
    "if_any_child_set",
];

fn nonempty_str(v: Option<&Value>) -> bool {
    matches!(v, Some(Value::String(s)) if !s.is_empty())
}

/// Validate an API definition. Returns error messages; empty means valid.
pub fn validate_definition(data: &Value) -> Vec<String> {
    let mut errors = Vec::new();
    let obj = match data.as_object() {
        Some(o) => o,
        None => return vec!["Definition must be a mapping.".into()],
    };

    if !nonempty_str(obj.get("name")) {
        errors.push("Missing or empty 'name' field.".into());
    }

    validate_request(obj.get("request"), &mut errors);

    if let Some(presets) = obj.get("tool_presets") {
        if !presets.is_object() {
            errors.push("'tool_presets' must be a dict mapping name -> body fragment.".into());
        }
    }

    validate_response(obj.get("response"), &mut errors);
    validate_operation(obj.get("operation"), &mut errors);
    validate_files(obj.get("files"), &mut errors);

    errors
}

fn validate_request(req: Option<&Value>, errors: &mut Vec<String>) {
    let Some(req) = req.and_then(Value::as_object) else {
        errors.push("Missing 'request' block.".into());
        return;
    };

    if !nonempty_str(req.get("path")) {
        errors.push("Missing or empty 'request.path'.".into());
    }

    match req.get("body_schema") {
        Some(Value::Object(schema)) => {
            match schema.get("type").and_then(Value::as_str) {
                None | Some("object") => {}
                Some(_) => errors.push("'request.body_schema.type' must be 'object'.".into()),
            }
            match schema.get("properties") {
                None => {}
                Some(Value::Object(props)) => {
                    for (pname, pdef) in props {
                        let Some(pdef) = pdef.as_object() else {
                            errors.push(format!("Property {pname:?} must be a dict."));
                            continue;
                        };
                        if let Some(inc) = pdef.get("x-include") {
                            let ok = inc
                                .as_str()
                                .map(|s| VALID_INCLUDES.contains(&s))
                                .unwrap_or(false);
                            if !ok {
                                let mut valid: Vec<&str> = VALID_INCLUDES.to_vec();
                                valid.sort_unstable();
                                let got = inc
                                    .as_str()
                                    .map(str::to_string)
                                    .unwrap_or_else(|| inc.to_string());
                                errors.push(format!(
                                    "Property '{pname}': invalid x-include '{got}' \
                                     (expected one of: {})",
                                    valid.join(", ")
                                ));
                            }
                        }
                    }
                }
                Some(_) => errors.push("'request.body_schema.properties' must be a dict.".into()),
            }
        }
        _ => errors.push("Missing or invalid 'request.body_schema' (must be a dict).".into()),
    }

    // Legacy keys are rejected outright (no backward compat).
    for legacy in ["body", "parameters", "param_mapping", "tools", "tools_key"] {
        if req.contains_key(legacy) {
            errors.push(format!(
                "'request.{legacy}' is from the legacy schema; use 'request.body_schema' instead."
            ));
        }
    }
}

fn validate_response(resp: Option<&Value>, errors: &mut Vec<String>) {
    let Some(resp) = resp.and_then(Value::as_object) else {
        errors.push("Missing 'response' block.".into());
        return;
    };

    for legacy in ["text_path", "text_fallbacks"] {
        if resp.contains_key(legacy) {
            errors.push(format!(
                "'response.{legacy}' is from the legacy schema; use 'response.text_extract' instead."
            ));
        }
    }

    let te = resp.get("text_extract");
    let arts = resp.get("artifacts");
    if te.is_none() && arts.is_none() {
        errors.push(
            "response must define 'text_extract' (text output) or 'artifacts' (binary output)."
                .into(),
        );
    }
    match te {
        None => {}
        Some(Value::Object(te)) => {
            let has_path = nonempty_str(te.get("text_path"));
            let has_items = nonempty_str(te.get("items_path"));
            if has_path && has_items {
                errors.push(
                    "'response.text_extract' must specify either text_path (simple) OR \
                     items_path (walked), not both."
                        .into(),
                );
            } else if !has_path && !has_items {
                errors.push(
                    "'response.text_extract' must specify either text_path or items_path.".into(),
                );
            }
            for opt in ["item_filter", "content_filter"] {
                if let Some(v) = te.get(opt) {
                    if !v.is_object() {
                        errors.push(format!(
                            "'response.text_extract.{opt}' must be a dict of field=value pairs."
                        ));
                    }
                }
            }
        }
        Some(_) => errors.push("'response.text_extract' must be a dict.".into()),
    }
    if let Some(arts) = arts {
        if !arts.is_object() {
            errors.push("'response.artifacts' must be a dict.".into());
        }
    }
    if let Some(meta) = resp.get("metadata") {
        if !meta.is_object() {
            errors.push("'response.metadata' must be a dict.".into());
        }
    }
    validate_stream(resp.get("stream"), errors);
    validate_activity(resp.get("activity"), errors);
    validate_annotations(resp.get("annotations"), errors);
}

fn validate_stream(stream: Option<&Value>, errors: &mut Vec<String>) {
    let Some(stream) = stream else { return };
    match stream.as_object() {
        None => errors.push("'response.stream' must be a dict.".into()),
        Some(stream) => {
            if let Some(ae) = stream.get("activity_events") {
                match ae.as_object() {
                    None => errors.push("'response.stream.activity_events' must be a dict.".into()),
                    Some(ae) => {
                        for (evname, msg) in ae {
                            if !msg.is_string() {
                                errors.push(format!(
                                    "'response.stream.activity_events.{evname}' must be a string."
                                ));
                            }
                        }
                    }
                }
            }
        }
    }
}

fn validate_activity(act: Option<&Value>, errors: &mut Vec<String>) {
    let Some(act) = act else { return };
    match act.as_object() {
        None => errors.push("'response.activity' must be a dict.".into()),
        Some(act) => {
            if let Some(ip) = act.get("items_path") {
                if !ip.is_string() {
                    errors.push("'response.activity.items_path' must be a string.".into());
                }
            }
            if let Some(sbt) = act.get("summarize_by_type") {
                match sbt.as_object() {
                    None => {
                        errors.push("'response.activity.summarize_by_type' must be a dict.".into())
                    }
                    Some(sbt) => {
                        for (tname, label) in sbt {
                            if !label.is_string() {
                                errors.push(format!(
                                    "'response.activity.summarize_by_type.{tname}' must be a string."
                                ));
                            }
                        }
                    }
                }
            }
        }
    }
}

fn validate_annotations(ann: Option<&Value>, errors: &mut Vec<String>) {
    let Some(ann) = ann else { return };
    let Some(ann) = ann.as_object() else {
        errors.push("'response.annotations' must be a dict.".into());
        return;
    };
    let scope = ann
        .get("scope")
        .and_then(Value::as_str)
        .unwrap_or("text_items");
    if scope != "text_items" && scope != "response_root" {
        errors.push(format!(
            "'response.annotations.scope' must be 'text_items' or 'response_root' (got {scope:?})."
        ));
    }
    match ann.get("types") {
        None => {}
        Some(Value::Object(types)) => {
            for (tname, tcfg) in types {
                let Some(tcfg) = tcfg.as_object() else {
                    errors.push(format!(
                        "'response.annotations.types.{tname}' must be a dict (e.g. {{format: ...}})."
                    ));
                    continue;
                };
                if let Some(f) = tcfg.get("format") {
                    if !f.is_string() {
                        errors.push(format!(
                            "'response.annotations.types.{tname}.format' must be a string."
                        ));
                    }
                }
            }
        }
        Some(_) => errors
            .push("'response.annotations.types' must be a dict mapping type → format spec.".into()),
    }
}

fn validate_operation(op: Option<&Value>, errors: &mut Vec<String>) {
    let Some(op) = op else { return };
    let Some(op) = op.as_object() else {
        errors.push("'operation' must be a dict.".into());
        return;
    };
    if let Some(poll) = op.get("poll") {
        match poll.as_object() {
            None => errors.push("'operation.poll' must be a dict.".into()),
            Some(poll) => {
                if let Some(pt) = poll.get("path_template") {
                    if !pt.is_string() {
                        errors.push("'operation.poll.path_template' must be a string.".into());
                    }
                }
            }
        }
    }
    if let Some(sm) = op.get("status_map") {
        if !sm.is_object() {
            errors.push("'operation.status_map' must be a dict.".into());
        }
    }
}

fn validate_files(files: Option<&Value>, errors: &mut Vec<String>) {
    let Some(files) = files else { return };
    let Some(files) = files.as_object() else {
        errors.push("'files' must be a dict.".into());
        return;
    };
    if let Some(up) = files.get("upload") {
        match up.as_object() {
            None => errors.push("'files.upload' must be a dict.".into()),
            Some(up) => {
                if let Some(p) = up.get("path") {
                    if !p.is_string() {
                        errors.push("'files.upload.path' must be a string.".into());
                    }
                }
            }
        }
    }
}

/// Walk a definition's `body_schema` and return a flat list of CLI flag specs
/// (one per property carrying an `x-cli` block), recursing into nested objects.
/// Each spec mirrors the CLI's `cli_flags_for`: flag, alias, path, type, enum,
/// default, description, repeat, invert, enum_map.
pub fn cli_flags(api_def: &Value) -> Vec<Value> {
    let mut flags = Vec::new();
    let props = api_def
        .get("request")
        .and_then(|r| r.get("body_schema"))
        .and_then(|b| b.get("properties"))
        .and_then(Value::as_object);
    if let Some(props) = props {
        collect_flags(props, &mut flags, "");
    }
    flags
}

fn collect_flags(properties: &serde_json::Map<String, Value>, out: &mut Vec<Value>, prefix: &str) {
    for (name, schema) in properties {
        let Some(schema) = schema.as_object() else {
            continue;
        };
        let path = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}.{name}")
        };
        if let Some(cli) = schema.get("x-cli").and_then(Value::as_object) {
            let description = schema
                .get("description")
                .filter(|v| !v.is_null())
                .or_else(|| cli.get("description"))
                .cloned()
                .unwrap_or(Value::Null);
            out.push(serde_json::json!({
                "flag": cli.get("flag").cloned().unwrap_or(Value::Null),
                "alias": cli.get("alias").cloned().unwrap_or(Value::Null),
                "path": path,
                "type": schema.get("type").cloned().unwrap_or(Value::Null),
                "enum": schema.get("enum").cloned().unwrap_or(Value::Null),
                "default": schema.get("default").cloned().unwrap_or(Value::Null),
                "description": description,
                "repeat": cli.get("repeat").and_then(Value::as_bool).unwrap_or(false),
                "invert": cli.get("invert").and_then(Value::as_bool).unwrap_or(false),
                "enum_map": cli.get("enum_map").cloned().unwrap_or(Value::Null),
            }));
        }
        if schema.get("type").and_then(Value::as_str) == Some("object") {
            if let Some(nested) = schema.get("properties").and_then(Value::as_object) {
                collect_flags(nested, out, &path);
            }
        }
    }
}

/// The sorted names of a definition's tool presets.
pub fn tool_preset_names(api_def: &Value) -> Vec<String> {
    let mut names: Vec<String> = api_def
        .get("tool_presets")
        .and_then(Value::as_object)
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default();
    names.sort();
    names
}

/// Build a live-preview of a definition for the editor: validation result, the
/// exposed CLI flags and tool presets, and — when the definition is valid — a
/// sample composed request body (or a `body_error`). A pure port of the CLI's
/// `api preview` JSON output.
pub fn preview_definition(def: &Value, prompt: &str, model: &str, role: &str) -> Value {
    let errors = validate_definition(def);
    let mut result = serde_json::json!({
        "name": def.get("name").cloned().unwrap_or(Value::String(String::new())),
        "description": def.get("description").cloned().unwrap_or(Value::String(String::new())),
        "valid": errors.is_empty(),
        "errors": errors.clone(),
        "flags": cli_flags(def),
        "presets": tool_preset_names(def),
    });
    if errors.is_empty() {
        // Bind the sample prompt the way a real request does: as a canonical
        // message lowered through the definition's `content_mapping`, so
        // `{{content}}` templates render (the payload keeps prompt/role too).
        let message = crate::message_ir::message_from_dict(&serde_json::json!({
            "role": role,
            "content": [{ "type": "text", "text": prompt }],
        }));
        let mut prompt_payload = serde_json::json!({ "prompt": prompt, "role": role });
        if let Value::Object(lowered) =
            crate::message_ir::lower_message(&message, def.get("content_mapping"))
        {
            prompt_payload.as_object_mut().unwrap().extend(lowered);
        }
        let overrides = serde_json::json!({ "model": model });
        match crate::compose::compose_request_body(
            def,
            &prompt_payload,
            &overrides,
            &[],
            &[],
            &[],
            false,
        ) {
            Ok(body) => {
                result["body"] = body;
            }
            Err(e) => {
                result["body_error"] = Value::String(e);
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn minimal() -> Value {
        json!({
            "name": "x",
            "request": { "path": "/v1/x", "body_schema": { "type": "object" } },
            "response": { "text_extract": { "text_path": "a.b" } }
        })
    }

    #[test]
    fn valid_definition_has_no_errors() {
        assert!(validate_definition(&minimal()).is_empty());
    }

    #[test]
    fn flags_missing_core_blocks() {
        let errors = validate_definition(&json!({}));
        assert!(errors.iter().any(|e| e.contains("'name'")));
        assert!(errors.iter().any(|e| e.contains("Missing 'request' block")));
        assert!(errors
            .iter()
            .any(|e| e.contains("Missing 'response' block")));
    }

    #[test]
    fn rejects_legacy_and_bad_text_extract() {
        let mut def = minimal();
        def["request"]["body"] = json!({});
        def["response"]["text_extract"] = json!({ "text_path": "a", "items_path": "b" });
        let errors = validate_definition(&def);
        assert!(errors.iter().any(|e| e.contains("legacy schema")));
        assert!(errors.iter().any(|e| e.contains("not both")));
    }

    #[test]
    fn rejects_bad_x_include() {
        let mut def = minimal();
        def["request"]["body_schema"]["properties"] = json!({ "p": { "x-include": "sometimes" } });
        let errors = validate_definition(&def);
        assert!(errors.iter().any(|e| e.contains("invalid x-include")));
    }

    #[test]
    fn requires_text_extract_or_artifacts() {
        let mut def = minimal();
        def["response"] = json!({ "metadata": {} });
        let errors = validate_definition(&def);
        assert!(errors
            .iter()
            .any(|e| e.contains("text_extract") && e.contains("artifacts")));
    }

    #[test]
    fn cli_flags_collects_x_cli_properties_recursively() {
        let def = json!({
            "request": { "path": "/x", "body_schema": { "type": "object", "properties": {
                "temperature": { "type": "number", "x-cli": { "flag": "--temperature", "alias": "-t" } },
                "nested": { "type": "object", "properties": {
                    "depth": { "type": "integer", "x-cli": { "flag": "--depth", "repeat": true } }
                }},
                "plain": { "type": "string" }
            }}}
        });
        let flags = cli_flags(&def);
        assert_eq!(flags.len(), 2);
        assert_eq!(flags[0]["flag"], "--temperature");
        assert_eq!(flags[0]["alias"], "-t");
        assert_eq!(flags[0]["path"], "temperature");
        assert_eq!(flags[1]["flag"], "--depth");
        assert_eq!(flags[1]["path"], "nested.depth");
        assert_eq!(flags[1]["repeat"], true);
    }

    #[test]
    fn tool_preset_names_sorted() {
        let def = json!({ "tool_presets": { "web": {}, "arith": {} } });
        assert_eq!(tool_preset_names(&def), ["arith", "web"]);
        assert!(tool_preset_names(&json!({})).is_empty());
    }

    #[test]
    fn preview_valid_definition_composes_body() {
        let out = preview_definition(&minimal(), "hello", "m1", "user");
        assert_eq!(out["valid"], true);
        assert_eq!(out["errors"].as_array().unwrap().len(), 0);
        assert!(out.get("body").is_some());
        assert!(out.get("body_error").is_none());
    }

    #[test]
    fn preview_invalid_definition_skips_body() {
        let out = preview_definition(&json!({ "name": "x" }), "hi", "m", "user");
        assert_eq!(out["valid"], false);
        assert!(!out["errors"].as_array().unwrap().is_empty());
        assert!(out.get("body").is_none());
    }
}
