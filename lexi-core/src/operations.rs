//! Async operation lifecycle (pure interpretation of an `operation` block).
//!
//! Mirrors `lexi_cli.operations`. Reads an operation handle/status from a submit
//! or poll response, classifies it into a canonical state, and builds the poll
//! path. The polling loop and HTTP live in the host. See
//! `docs/spec/operations-v0.md`.

use serde_json::{Map, Value};

use crate::path::get_path;

pub const CANONICAL_STATES: [&str; 4] = ["pending", "running", "succeeded", "failed"];

fn terminal(state: &str) -> bool {
    state == "succeeded" || state == "failed"
}

/// True if `state` is a terminal canonical state (`succeeded`/`failed`).
pub fn is_terminal(state: &str) -> bool {
    terminal(state)
}

/// Map a raw provider status onto a canonical state (`unknown` if unmatched).
pub fn classify_status(status: &Value, status_map: Option<&Value>) -> String {
    if status.is_null() {
        return "unknown".to_string();
    }
    if let Some(map) = status_map.and_then(Value::as_object) {
        for state in CANONICAL_STATES {
            if let Some(Value::Array(vals)) = map.get(state) {
                if vals.iter().any(|v| v == status) {
                    return state.to_string();
                }
            }
        }
    }
    "unknown".to_string()
}

/// Read `{id, status, state, done}` from a submit or poll response.
pub fn read_operation(response: &Value, operation_cfg: Option<&Value>) -> Value {
    let mut m = Map::new();
    let cfg = match operation_cfg.and_then(Value::as_object) {
        Some(c) => c,
        None => {
            m.insert("id".into(), Value::Null);
            m.insert("status".into(), Value::Null);
            m.insert("state".into(), Value::from("unknown"));
            m.insert("done".into(), Value::from(false));
            return Value::Object(m);
        }
    };
    let id_path = cfg.get("id_path").and_then(Value::as_str).unwrap_or("id");
    let status_path = cfg
        .get("status_path")
        .and_then(Value::as_str)
        .unwrap_or("status");
    let id = get_path(response, id_path).cloned().unwrap_or(Value::Null);
    let status = get_path(response, status_path)
        .cloned()
        .unwrap_or(Value::Null);
    let state = classify_status(&status, cfg.get("status_map"));
    let done = terminal(&state);

    m.insert("id".into(), id);
    m.insert("status".into(), status);
    m.insert("state".into(), Value::from(state));
    m.insert("done".into(), Value::from(done));
    Value::Object(m)
}

/// Build the poll path for `operation_id` by substituting `{id}`.
pub fn poll_path(operation_cfg: &Value, operation_id: &Value) -> String {
    let template = operation_cfg
        .get("poll")
        .and_then(|p| p.get("path_template"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let id_str = match operation_id {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    template.replace("{id}", &id_str)
}
