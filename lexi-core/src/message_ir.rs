//! Canonical message intermediate representation (IR).
//!
//! Mirrors `lexi_cli.message_ir`. A message is an ordered list of typed content
//! parts; v0 has only [`ContentPart::Text`], so a text message lowers losslessly
//! to the `{prompt, role}` bindings the API-definition templates consume. See
//! `docs/spec/message-ir-v0.md`.

use serde_json::{Map, Value};

pub const DEFAULT_ROLE: &str = "user";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentPart {
    Text { text: String },
}

impl ContentPart {
    pub fn to_value(&self) -> Value {
        match self {
            ContentPart::Text { text } => {
                let mut m = Map::new();
                m.insert("type".to_string(), Value::from("text"));
                m.insert("text".to_string(), Value::from(text.clone()));
                Value::Object(m)
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub role: String,
    pub content: Vec<ContentPart>,
}

impl Message {
    pub fn to_value(&self) -> Value {
        let mut m = Map::new();
        m.insert("role".to_string(), Value::from(self.role.clone()));
        m.insert(
            "content".to_string(),
            Value::Array(self.content.iter().map(ContentPart::to_value).collect()),
        );
        Value::Object(m)
    }
}

/// Build a single-text-part message (the v0 common case).
pub fn text_message(text: &str, role: &str) -> Message {
    Message {
        role: role.to_string(),
        content: vec![ContentPart::Text {
            text: text.to_string(),
        }],
    }
}

/// Parse a content-part dict. v0 supports only `type: "text"`.
pub fn part_from_dict(data: &Value) -> Result<ContentPart, String> {
    let part_type = data.get("type").and_then(Value::as_str).unwrap_or("text");
    if part_type == "text" {
        let text = data.get("text").and_then(Value::as_str).unwrap_or("");
        Ok(ContentPart::Text {
            text: text.to_string(),
        })
    } else {
        Err(format!(
            "unsupported content part type {part_type:?} (v0 supports only \"text\")"
        ))
    }
}

/// Parse a canonical message dict (content: list | string sugar | absent).
pub fn message_from_dict(data: &Value) -> Message {
    let role = data
        .get("role")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_ROLE)
        .to_string();
    let content = data.get("content");
    let parts = match content {
        Some(Value::String(s)) => vec![ContentPart::Text { text: s.clone() }],
        Some(Value::Array(a)) => a
            .iter()
            .map(|p| {
                if p.is_object() {
                    part_from_dict(p).unwrap_or_else(|_| ContentPart::Text {
                        text: p.to_string(),
                    })
                } else {
                    ContentPart::Text {
                        text: value_to_plain_string(p),
                    }
                }
            })
            .collect(),
        Some(Value::Null) | None => vec![ContentPart::Text {
            text: String::new(),
        }],
        Some(other) => vec![ContentPart::Text {
            text: value_to_plain_string(other),
        }],
    };
    Message {
        role,
        content: parts,
    }
}

fn value_to_plain_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Upgrade a legacy prompt payload (`{prompt, role, ...}`) into a Message.
pub fn normalize_prompt_payload(payload: &Value) -> Message {
    let text = payload.get("prompt").and_then(Value::as_str).unwrap_or("");
    let role = payload
        .get("role")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_ROLE);
    text_message(text, role)
}

/// Concatenate the text of all text parts (joiner `""`).
pub fn text_of(message: &Message) -> String {
    message
        .content
        .iter()
        .map(|p| match p {
            ContentPart::Text { text } => text.as_str(),
        })
        .collect()
}

/// Lower a text-only message to `{prompt, role}` bindings.
pub fn to_prompt_bindings(message: &Message) -> Result<Value, String> {
    // All parts are text in v0; this match exists so new variants force a decision.
    for part in &message.content {
        match part {
            ContentPart::Text { .. } => {}
        }
    }
    let mut m = Map::new();
    m.insert("role".to_string(), Value::from(message.role.clone()));
    m.insert("prompt".to_string(), Value::from(text_of(message)));
    Ok(Value::Object(m))
}
