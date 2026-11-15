//! Schema-driven request-body composition.
//!
//! Mirrors `lexi_cli.api_compose.compose_request_body` and its helpers.

use serde_json::{Map, Value};

use crate::path::{set_path, unset_path};
use crate::template::render_template;
use crate::validate::validate;

const VALID_INCLUDES: [&str; 5] = [
    "always",
    "if_set",
    "if_truthy",
    "if_non_empty",
    "if_any_child_set",
];

use std::sync::OnceLock;

fn empty_map() -> Map<String, Value> {
    Map::new()
}

fn empty_static() -> &'static Map<String, Value> {
    static EMPTY: OnceLock<Map<String, Value>> = OnceLock::new();
    EMPTY.get_or_init(Map::new)
}

fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().map(|f| f != 0.0).unwrap_or(true),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(m) => !m.is_empty(),
    }
}

fn should_include(value: &Value, mode: &str) -> bool {
    match mode {
        "always" => true,
        "if_set" => !value.is_null(),
        "if_truthy" => truthy(value),
        "if_non_empty" => match value {
            Value::Null => false,
            Value::String(s) => !s.is_empty(),
            Value::Array(a) => !a.is_empty(),
            Value::Object(m) => !m.is_empty(),
            _ => true, // non-sized (number/bool): Python len() raises -> True
        },
        "if_any_child_set" => match value {
            Value::Object(m) => m.values().any(|v| !v.is_null()),
            Value::Array(a) => !a.is_empty(),
            _ => !value.is_null(),
        },
        _ => !value.is_null(),
    }
}

fn default_include(prop_schema: &Map<String, Value>) -> &'static str {
    if prop_schema.contains_key("default")
        || prop_schema.contains_key("x-bind")
        || prop_schema.contains_key("x-render")
    {
        "always"
    } else {
        "if_set"
    }
}

/// Resolve a property to `(value, present)`; `present = false` means omit.
fn resolve_property(
    name: &str,
    schema: &Map<String, Value>,
    prompt_payload: &Map<String, Value>,
    overrides: &Map<String, Value>,
    bindings: &Map<String, Value>,
) -> (Value, bool) {
    if let Some(bind) = schema.get("x-bind") {
        let var = bind
            .get("from")
            .and_then(Value::as_str)
            .unwrap_or(name)
            .to_string();
        if let Some(v) = bindings.get(&var) {
            if !v.is_null() {
                return (v.clone(), true);
            }
        }
        if let Some(d) = schema.get("default") {
            return (d.clone(), true);
        }
        return (Value::Null, false);
    }

    if let Some(render) = schema.get("x-render") {
        let template = render.get("template").cloned().unwrap_or(Value::Null);
        return (render_template(&template, bindings), true);
    }

    if let Some(v) = overrides.get(name) {
        if !v.is_null() {
            return (v.clone(), true);
        }
    }
    if let Some(v) = prompt_payload.get(name) {
        if !v.is_null() {
            return (v.clone(), true);
        }
    }
    if let Some(d) = schema.get("default") {
        return (d.clone(), true);
    }

    (Value::Null, false)
}

/// Deep-merge `overlay` into `base`; **lists are concatenated** (preset merge).
fn deep_merge_concat(base: &mut Map<String, Value>, overlay: &Map<String, Value>) {
    for (k, v) in overlay {
        match (base.get_mut(k), v) {
            (Some(Value::Object(bo)), Value::Object(vo)) => deep_merge_concat(bo, vo),
            (Some(Value::Array(ba)), Value::Array(va)) => {
                ba.extend(va.iter().cloned());
            }
            _ => {
                base.insert(k.clone(), v.clone());
            }
        }
    }
}

/// Compose a JSON request body from a schema-driven API definition.
#[allow(clippy::too_many_arguments)]
pub fn compose_request_body(
    api_def: &Value,
    prompt_payload: &Value,
    overrides: &Value,
    tool_presets_selected: &[String],
    set_paths: &[(String, Value)],
    unset_paths: &[String],
    skip_validation: bool,
) -> Result<Value, String> {
    let request = api_def.get("request").cloned().unwrap_or(Value::Null);
    let body_schema = request.get("body_schema").cloned().unwrap_or(Value::Null);
    if !body_schema.is_object() || body_schema.as_object().unwrap().is_empty() {
        return Err("API definition is missing request.body_schema".to_string());
    }

    let pp = prompt_payload
        .as_object()
        .cloned()
        .unwrap_or_else(empty_map);
    let ov = overrides.as_object().cloned().unwrap_or_else(empty_map);

    let mut bindings = pp.clone();
    for (k, v) in &ov {
        if !v.is_null() {
            bindings.insert(k.clone(), v.clone());
        }
    }

    let mut body = Map::new();
    let empty = empty_static();
    let properties = body_schema
        .get("properties")
        .and_then(Value::as_object)
        .unwrap_or(empty);

    for (prop_name, prop_schema_val) in properties {
        let prop_schema = match prop_schema_val.as_object() {
            Some(m) => m,
            None => continue,
        };
        let (value, present) = resolve_property(prop_name, prop_schema, &pp, &ov, &bindings);
        if !present {
            continue;
        }
        let mode = prop_schema
            .get("x-include")
            .and_then(Value::as_str)
            .map(String::from)
            .unwrap_or_else(|| default_include(prop_schema).to_string());
        if !VALID_INCLUDES.contains(&mode.as_str()) {
            return Err(format!(
                "property {prop_name:?}: invalid x-include {mode:?}"
            ));
        }
        if !should_include(&value, &mode) {
            continue;
        }
        body.insert(prop_name.clone(), value);
    }

    let presets_block = api_def
        .get("tool_presets")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_else(empty_map);
    for preset_name in tool_presets_selected {
        match presets_block.get(preset_name) {
            Some(Value::Object(fragment)) => deep_merge_concat(&mut body, fragment),
            Some(_) => return Err(format!("tool preset {preset_name:?} is not an object")),
            None => return Err(format!("unknown tool preset {preset_name:?}")),
        }
    }

    let mut body_value = Value::Object(body);
    for (path, value) in set_paths {
        set_path(&mut body_value, path, value.clone());
    }
    for path in unset_paths {
        unset_path(&mut body_value, path);
    }

    if !skip_validation {
        let errors = validate(&body_schema, &body_value, "");
        if !errors.is_empty() {
            return Err(format!("request body validation failed: {errors:?}"));
        }
    }

    Ok(body_value)
}
