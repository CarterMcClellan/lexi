//! Capability registry: what a provider/model can accept and produce.
//!
//! Mirrors `lexi_cli.capabilities`. A descriptor declares input/output
//! modalities (+ optional limits) for an API family, with optional per-model
//! overrides. See `docs/spec/capabilities-v0.md`.

use std::collections::HashSet;

use serde_json::{Map, Value};

use crate::message_ir::Message;

fn array_or_empty(obj: Option<&Map<String, Value>>, key: &str) -> Vec<Value> {
    obj.and_then(|m| m.get(key))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

/// Resolve the effective capability descriptor for an API family + model.
///
/// A per-model override replaces `input`/`output` wholesale and merges `limits`
/// key-by-key.
pub fn resolve_capabilities(api_def: &Value, model_capabilities: Option<&Value>) -> Value {
    let base = api_def.get("capabilities").and_then(Value::as_object);
    let mut input = array_or_empty(base, "input");
    let mut output = array_or_empty(base, "output");
    let mut limits = base
        .and_then(|m| m.get("limits"))
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();

    if let Some(mc) = model_capabilities.and_then(Value::as_object) {
        if mc.contains_key("input") {
            input = mc
                .get("input")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
        }
        if mc.contains_key("output") {
            output = mc
                .get("output")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
        }
        if let Some(ml) = mc.get("limits").and_then(Value::as_object) {
            for (k, v) in ml {
                limits.insert(k.clone(), v.clone());
            }
        }
    }

    let mut out = Map::new();
    out.insert("input".into(), Value::Array(input));
    out.insert("output".into(), Value::Array(output));
    out.insert("limits".into(), Value::Object(limits));
    Value::Object(out)
}

/// True if the capability descriptor accepts `modality` as input.
pub fn supports_input(cap: &Value, modality: &str) -> bool {
    cap.get("input")
        .and_then(Value::as_array)
        .map(|a| a.iter().any(|v| v.as_str() == Some(modality)))
        .unwrap_or(false)
}

/// True if the capability descriptor can produce `modality` as output.
pub fn supports_output(cap: &Value, modality: &str) -> bool {
    cap.get("output")
        .and_then(Value::as_array)
        .map(|a| a.iter().any(|v| v.as_str() == Some(modality)))
        .unwrap_or(false)
}

/// Return errors for any content part whose modality the model can't accept
/// (one per unsupported modality, first-seen order).
pub fn validate_message(cap: &Value, message: &Message) -> Vec<String> {
    let mut errors = Vec::new();
    let mut seen: HashSet<&str> = HashSet::new();
    for part in &message.content {
        let modality = part.type_name();
        if seen.contains(modality) {
            continue;
        }
        if !supports_input(cap, modality) {
            seen.insert(modality);
            errors.push(format!("model does not accept {modality} input"));
        }
    }
    errors
}
