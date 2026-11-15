//! Canonical message intermediate representation (IR).
//!
//! Mirrors `lexi_cli.message_ir`. A message is an ordered list of typed content
//! parts ([`ContentPart::Text`], [`ContentPart::Image`]); content is lowered to
//! a provider's wire shape via that provider's `content_mapping`, so text chat
//! stays byte-identical while image input is additive. See
//! `docs/spec/message-ir-v0.md`.

use serde_json::{Map, Value};

use crate::template::render_template;

pub const DEFAULT_ROLE: &str = "user";
pub const TEXT: &str = "text";
pub const IMAGE: &str = "image";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentPart {
    Text {
        text: String,
    },
    Image {
        url: Option<String>,
        data: Option<String>, // base64-encoded image bytes
        media_type: Option<String>,
    },
}

impl ContentPart {
    pub fn type_name(&self) -> &'static str {
        match self {
            ContentPart::Text { .. } => TEXT,
            ContentPart::Image { .. } => IMAGE,
        }
    }

    /// For images: `"base64"` when inline `data` is present, else `"url"`.
    pub fn source_kind(&self) -> Option<&'static str> {
        match self {
            ContentPart::Image { data, .. } => Some(if data.is_some() { "base64" } else { "url" }),
            _ => None,
        }
    }

    pub fn to_value(&self) -> Value {
        let mut m = Map::new();
        match self {
            ContentPart::Text { text } => {
                m.insert("type".into(), Value::from(TEXT));
                m.insert("text".into(), Value::from(text.clone()));
            }
            ContentPart::Image {
                url,
                data,
                media_type,
            } => {
                m.insert("type".into(), Value::from(IMAGE));
                if let Some(u) = url {
                    m.insert("url".into(), Value::from(u.clone()));
                }
                if let Some(d) = data {
                    m.insert("data".into(), Value::from(d.clone()));
                }
                if let Some(mt) = media_type {
                    m.insert("media_type".into(), Value::from(mt.clone()));
                }
            }
        }
        Value::Object(m)
    }

    /// Variables used to render this part's content-mapping template.
    fn render_vars(&self) -> Map<String, Value> {
        let mut m = Map::new();
        match self {
            ContentPart::Text { text } => {
                m.insert("text".into(), Value::from(text.clone()));
            }
            ContentPart::Image {
                url,
                data,
                media_type,
            } => {
                let mt = media_type.clone().unwrap_or_default();
                let data_uri = match data {
                    Some(d) => format!("data:{mt};base64,{d}"),
                    None => url.clone().unwrap_or_default(),
                };
                m.insert("url".into(), Value::from(url.clone().unwrap_or_default()));
                m.insert("data".into(), Value::from(data.clone().unwrap_or_default()));
                m.insert("media_type".into(), Value::from(mt));
                m.insert("data_uri".into(), Value::from(data_uri));
            }
        }
        m
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
        m.insert("role".into(), Value::from(self.role.clone()));
        m.insert(
            "content".into(),
            Value::Array(self.content.iter().map(ContentPart::to_value).collect()),
        );
        Value::Object(m)
    }
}

/// Build a single-text-part message (the common case).
pub fn text_message(text: &str, role: &str) -> Message {
    Message {
        role: role.to_string(),
        content: vec![ContentPart::Text {
            text: text.to_string(),
        }],
    }
}

/// Parse a content-part dict (`type: text` or `type: image`).
pub fn part_from_dict(data: &Value) -> Result<ContentPart, String> {
    let part_type = data.get("type").and_then(Value::as_str).unwrap_or(TEXT);
    match part_type {
        TEXT => Ok(ContentPart::Text {
            text: data
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
        }),
        IMAGE => {
            let media_type = data
                .get("media_type")
                .and_then(Value::as_str)
                .map(String::from);
            if let Some(d) = data.get("data").and_then(Value::as_str) {
                Ok(ContentPart::Image {
                    url: None,
                    data: Some(d.to_string()),
                    media_type,
                })
            } else {
                Ok(ContentPart::Image {
                    url: Some(
                        data.get("url")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string(),
                    ),
                    data: None,
                    media_type,
                })
            }
        }
        other => Err(format!(
            "unsupported content part type {other:?} (supported: \"text\", \"image\")"
        )),
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
        .filter_map(|p| match p {
            ContentPart::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

/// Lower a text-only message to `{prompt, role}` bindings (no provider mapping).
pub fn to_prompt_bindings(message: &Message) -> Result<Value, String> {
    for part in &message.content {
        if !matches!(part, ContentPart::Text { .. }) {
            return Err(format!(
                "to_prompt_bindings supports only text parts; got {}",
                part.type_name()
            ));
        }
    }
    let mut m = Map::new();
    m.insert("role".into(), Value::from(message.role.clone()));
    m.insert("prompt".into(), Value::from(text_of(message)));
    Ok(Value::Object(m))
}

/// Pick a part's wire template: a `by_source` sub-template keyed by the part's
/// `source_kind` (image url vs base64), else the flat `template`.
fn select_template<'a>(part: &ContentPart, spec: &'a Value) -> Option<&'a Value> {
    match spec.get("by_source") {
        Some(by_source) => part
            .source_kind()
            .and_then(|kind| by_source.get(kind))
            .and_then(|sub| sub.get("template")),
        None => spec.get("template"),
    }
}

/// Render one content part into a provider's wire shape via its template.
fn lower_content_part(part: &ContentPart, parts_mapping: &Map<String, Value>) -> Value {
    match parts_mapping
        .get(part.type_name())
        .and_then(|spec| select_template(part, spec))
    {
        Some(template) => render_template(template, &part.render_vars()),
        None => part.to_value(), // no mapping: best-effort passthrough
    }
}

/// Lower a Message to template bindings, provider-aware.
///
/// Returns `{role, content, prompt}` (content a string when `text_only_as_string`
/// and the message is a single text part, else a list of lowered parts). Falls
/// back to `{role, prompt}` when `content_mapping` is absent.
pub fn lower_message(message: &Message, content_mapping: Option<&Value>) -> Value {
    let mapping = match content_mapping.and_then(Value::as_object) {
        Some(m) => m,
        None => {
            let mut out = Map::new();
            out.insert("role".into(), Value::from(message.role.clone()));
            out.insert("prompt".into(), Value::from(text_of(message)));
            return Value::Object(out);
        }
    };

    let empty = Map::new();
    let parts_mapping = mapping
        .get("parts")
        .and_then(Value::as_object)
        .unwrap_or(&empty);
    let text_only_as_string = mapping
        .get("text_only_as_string")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let content = if text_only_as_string
        && message.content.len() == 1
        && matches!(message.content[0], ContentPart::Text { .. })
    {
        match &message.content[0] {
            ContentPart::Text { text } => Value::from(text.clone()),
            _ => unreachable!(),
        }
    } else {
        Value::Array(
            message
                .content
                .iter()
                .map(|p| lower_content_part(p, parts_mapping))
                .collect(),
        )
    };

    let mut out = Map::new();
    out.insert("role".into(), Value::from(message.role.clone()));
    out.insert("content".into(), content);
    out.insert("prompt".into(), Value::from(text_of(message)));
    Value::Object(out)
}
