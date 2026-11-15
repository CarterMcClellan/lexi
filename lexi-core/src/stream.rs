//! SSE streaming: parse an event block and classify it against `response.stream`.
//!
//! Pure interpretation only — the host reads the byte stream and splits it into
//! event blocks; this module turns each (event-name, data) pair into a typed
//! outcome (`delta` / `done` / `activity` / `other`). See the `stream` block in
//! the API definitions and `docs/spec/api-definition-v0.md` §5.5.

use serde_json::{json, Map, Value};

use crate::path::get_path;

/// Parse one SSE event block into `{ event: <name|null>, data: <string> }`.
///
/// `event:` sets the name; `data:` lines (one optional leading space stripped)
/// are joined with `\n`.
pub fn parse_sse_block(block: &str) -> Value {
    let mut event: Option<String> = None;
    let mut data_lines: Vec<String> = Vec::new();
    for line in block.lines() {
        if let Some(rest) = line.strip_prefix("event:") {
            event = Some(rest.trim().to_string());
        } else if let Some(rest) = line.strip_prefix("data:") {
            data_lines.push(rest.strip_prefix(' ').unwrap_or(rest).to_string());
        }
    }
    let mut m = Map::new();
    m.insert(
        "event".into(),
        event.map(Value::from).unwrap_or(Value::Null),
    );
    m.insert("data".into(), Value::from(data_lines.join("\n")));
    Value::Object(m)
}

/// Classify an SSE event against the `stream` config. Returns one of:
/// `{kind:"delta", text}`, `{kind:"done"}`, `{kind:"activity", label}`,
/// `{kind:"other"}`.
pub fn classify_event(event_name: Option<&str>, data: &str, stream_cfg: &Value) -> Value {
    let cfg = match stream_cfg.as_object() {
        Some(c) => c,
        None => return json!({ "kind": "other" }),
    };

    // Terminal: a literal data sentinel (Chat Completions `[DONE]`).
    if let Some(marker) = cfg.get("done_marker").and_then(Value::as_str) {
        if data.trim() == marker {
            return json!({ "kind": "done" });
        }
    }
    // Terminal: a named done event.
    if let Some(done_ev) = cfg.get("done_event").and_then(Value::as_str) {
        if event_name == Some(done_ev) {
            return json!({ "kind": "done" });
        }
    }
    // Activity: a named status event mapped to a label.
    if let Some(name) = event_name {
        if let Some(label) = cfg
            .get("activity_events")
            .and_then(Value::as_object)
            .and_then(|ae| ae.get(name))
            .and_then(Value::as_str)
        {
            return json!({ "kind": "activity", "label": label });
        }
    }
    // Delta: delta_event matches (a string event name, or null/absent = any),
    // and delta_path resolves to a string fragment in the event data.
    let matches_delta = match cfg.get("delta_event") {
        Some(Value::String(de)) => event_name == Some(de.as_str()),
        _ => true,
    };
    if matches_delta {
        if let Some(delta_path) = cfg.get("delta_path").and_then(Value::as_str) {
            if let Ok(data_json) = serde_json::from_str::<Value>(data) {
                if let Some(Value::String(frag)) = get_path(&data_json, delta_path) {
                    return json!({ "kind": "delta", "text": frag });
                }
            }
        }
    }
    json!({ "kind": "other" })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_block_named_event() {
        let v = parse_sse_block("event: content_block_delta\ndata: {\"x\":1}");
        assert_eq!(v["event"], "content_block_delta");
        assert_eq!(v["data"], "{\"x\":1}");
    }

    #[test]
    fn parse_block_unnamed_multi_data() {
        let v = parse_sse_block("data: a\ndata: b");
        assert!(v["event"].is_null());
        assert_eq!(v["data"], "a\nb");
    }
}
