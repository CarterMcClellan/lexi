//! Response extraction: text, activity summary, annotations, text-item lookup.
//!
//! Mirrors the `extract_*` / `summarize_activity` / `find_text_items` /
//! `render_annotations` functions in `lexi_cli.api_compose`.

use serde_json::{Map, Value};

use crate::path::get_path;

/// True iff every key/value in `filter` matches `item`. Empty filter matches
/// everything; a non-object item matches only an empty filter.
fn all_kv_match(item: &Value, filter: &Map<String, Value>) -> bool {
    if filter.is_empty() {
        return true;
    }
    let obj = match item.as_object() {
        Some(o) => o,
        None => return false,
    };
    filter
        .iter()
        .all(|(k, expected)| obj.get(k).unwrap_or(&Value::Null) == expected)
}

fn as_map(v: Option<&Value>) -> Map<String, Value> {
    v.and_then(Value::as_object).cloned().unwrap_or_default()
}

fn as_str_or<'a>(cfg: &'a Value, key: &str, default: &'a str) -> &'a str {
    cfg.get(key).and_then(Value::as_str).unwrap_or(default)
}

/// Extract typed output artifacts (e.g. generated images) from a response.
///
/// Each artifact is `{ "type": <type>, **present_fields }`; a field whose source
/// path resolves to null/absent is omitted. Returns `[]` when nothing matches.
pub fn extract_artifacts(response: &Value, cfg: Option<&Value>) -> Vec<Value> {
    let cfg = match cfg {
        Some(c) if c.is_object() => c,
        _ => return Vec::new(),
    };
    let items_path = match cfg.get("items_path").and_then(Value::as_str) {
        Some(p) if !p.is_empty() => p,
        _ => return Vec::new(),
    };
    let items = match get_path(response, items_path) {
        Some(Value::Array(a)) => a,
        _ => return Vec::new(),
    };
    let atype = cfg.get("type").and_then(Value::as_str).unwrap_or("file");
    let fields = cfg.get("fields").and_then(Value::as_object);

    let mut out = Vec::new();
    for item in items {
        let mut artifact = Map::new();
        artifact.insert("type".into(), Value::from(atype));
        if let Some(fields) = fields {
            for (out_name, src) in fields {
                if let Some(src) = src.as_str() {
                    if let Some(v) = get_path(item, src) {
                        if !v.is_null() {
                            artifact.insert(out_name.clone(), v.clone());
                        }
                    }
                }
            }
        }
        out.push(Value::Object(artifact));
    }
    out
}

/// Extract the assistant text per `text_extract` config (Mode A or Mode B).
pub fn extract_text(response: &Value, cfg: Option<&Value>) -> String {
    let cfg = match cfg {
        Some(c) if c.is_object() => c,
        _ => return String::new(),
    };

    // Mode A — simple path.
    if let Some(text_path) = cfg.get("text_path").and_then(Value::as_str) {
        if !text_path.is_empty() {
            return match get_path(response, text_path) {
                Some(Value::String(s)) => s.clone(),
                _ => String::new(),
            };
        }
    }

    // Mode B — walk and filter.
    let items_path = match cfg.get("items_path").and_then(Value::as_str) {
        Some(p) if !p.is_empty() => p,
        _ => return String::new(),
    };
    let items = match get_path(response, items_path) {
        Some(Value::Array(a)) => a,
        _ => return String::new(),
    };
    let item_filter = as_map(cfg.get("item_filter"));
    let content_path = cfg.get("content_path").and_then(Value::as_str);
    let content_filter = as_map(cfg.get("content_filter"));
    let text_field = as_str_or(cfg, "text_field", "text");
    let joiner = as_str_or(cfg, "joiner", "");

    let mut chunks: Vec<String> = Vec::new();
    for item in items {
        if !all_kv_match(item, &item_filter) {
            continue;
        }
        let contents_val = match content_path {
            Some(cp) => get_path(item, cp).cloned(),
            None => Some(item.clone()),
        };
        let contents = match contents_val {
            None | Some(Value::Null) => continue,
            Some(Value::Array(a)) => a,
            Some(other) => vec![other],
        };
        for c in &contents {
            if !all_kv_match(c, &content_filter) {
                continue;
            }
            if let Some(Value::String(s)) = c.as_object().and_then(|o| o.get(text_field)) {
                chunks.push(s.clone());
            }
        }
    }
    chunks.join(joiner)
}

/// Return the content nodes a Mode-B `text_extract` matched (empty for Mode A).
pub fn find_text_items(response: &Value, cfg: Option<&Value>) -> Vec<Value> {
    let cfg = match cfg {
        Some(c) if c.is_object() => c,
        _ => return Vec::new(),
    };
    if cfg
        .get("text_path")
        .and_then(Value::as_str)
        .is_some_and(|s| !s.is_empty())
    {
        return Vec::new();
    }
    let items_path = match cfg.get("items_path").and_then(Value::as_str) {
        Some(p) if !p.is_empty() => p,
        _ => return Vec::new(),
    };
    let items = match get_path(response, items_path) {
        Some(Value::Array(a)) => a,
        _ => return Vec::new(),
    };
    let item_filter = as_map(cfg.get("item_filter"));
    let content_path = cfg.get("content_path").and_then(Value::as_str);
    let content_filter = as_map(cfg.get("content_filter"));

    let mut matched = Vec::new();
    for item in items {
        if !all_kv_match(item, &item_filter) {
            continue;
        }
        let contents_val = match content_path {
            Some(cp) => get_path(item, cp).cloned(),
            None => Some(item.clone()),
        };
        let contents = match contents_val {
            None | Some(Value::Null) => continue,
            Some(Value::Array(a)) => a,
            Some(other) => vec![other],
        };
        for c in contents {
            if c.is_object() && all_kv_match(&c, &content_filter) {
                matched.push(c);
            }
        }
    }
    matched
}

/// One-line activity summary like `"1 web search · reasoning"`.
pub fn summarize_activity(response: &Value, cfg: Option<&Value>) -> String {
    let cfg = match cfg {
        Some(c) if c.is_object() => c,
        _ => return String::new(),
    };
    let types_cfg = match cfg.get("summarize_by_type").and_then(Value::as_object) {
        Some(m) if !m.is_empty() => m,
        _ => return String::new(),
    };
    let items_path = as_str_or(cfg, "items_path", "output");
    let items = match get_path(response, items_path) {
        Some(Value::Array(a)) => a,
        _ => return String::new(),
    };

    let mut counts: Map<String, Value> = Map::new();
    for item in items {
        if let Some(Value::String(t)) = item.as_object().and_then(|o| o.get("type")) {
            let n = counts.get(t).and_then(Value::as_i64).unwrap_or(0) + 1;
            counts.insert(t.clone(), Value::from(n));
        }
    }

    let mut parts: Vec<String> = Vec::new();
    for (type_name, label) in types_cfg {
        let n = counts.get(type_name).and_then(Value::as_i64).unwrap_or(0);
        if n <= 0 {
            continue;
        }
        if let Some(label) = label.as_str() {
            parts.push(label.replace("{count}", &n.to_string()));
        }
    }
    parts.join(" · ")
}

/// Collect annotations matched by `annotations_cfg`, in document order.
pub fn extract_annotations(
    response: &Value,
    cfg: Option<&Value>,
    text_items: &[Value],
) -> Vec<Value> {
    let cfg = match cfg {
        Some(c) if c.is_object() => c,
        _ => return Vec::new(),
    };
    let types_cfg = match cfg.get("types").and_then(Value::as_object) {
        Some(m) if !m.is_empty() => m,
        _ => return Vec::new(),
    };
    let path = as_str_or(cfg, "path", "annotations");
    let scope = as_str_or(cfg, "scope", "text_items");

    let sources: Vec<Value> = match scope {
        "response_root" => vec![response.clone()],
        "text_items" => text_items.to_vec(),
        _ => return Vec::new(),
    };

    let mut out = Vec::new();
    for src in &sources {
        let annotations = match get_path(src, path) {
            Some(Value::Array(a)) => a,
            _ => continue,
        };
        for ann in annotations {
            let t = match ann.as_object().and_then(|o| o.get("type")) {
                Some(Value::String(t)) => t,
                _ => continue,
            };
            if types_cfg.contains_key(t) {
                out.push(ann.clone());
            }
        }
    }
    out
}

fn fmt_val(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => {
            if *b {
                "True".to_string()
            } else {
                "False".to_string()
            }
        }
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// Substitute `{name}` placeholders from `ctx`; missing -> empty string.
/// UTF-8 safe: literal segments are copied as `&str` slices.
fn format_map(fmt: &str, ctx: &Map<String, Value>) -> String {
    let mut out = String::new();
    let mut rest = fmt;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after.find('}') {
            Some(close) => {
                let key = &after[..close];
                if let Some(v) = ctx.get(key) {
                    out.push_str(&fmt_val(v));
                }
                rest = &after[close + 1..];
            }
            None => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Render annotations using each type's `format` template (`{i}` auto-numbered).
pub fn render_annotations(annotations: &[Value], cfg: Option<&Value>) -> String {
    if annotations.is_empty() {
        return String::new();
    }
    let cfg = match cfg {
        Some(c) if c.is_object() => c,
        _ => return String::new(),
    };
    let types_cfg = as_map(cfg.get("types"));

    let mut lines = Vec::new();
    for (idx, ann) in annotations.iter().enumerate() {
        let i = idx + 1;
        let t = ann
            .as_object()
            .and_then(|o| o.get("type"))
            .and_then(Value::as_str)
            .unwrap_or("");
        let fmt = types_cfg
            .get(t)
            .and_then(|tc| tc.get("format"))
            .and_then(Value::as_str);
        let mut ctx = Map::new();
        ctx.insert("i".to_string(), Value::from(i as i64));
        if let Some(o) = ann.as_object() {
            for (k, v) in o {
                ctx.insert(k.clone(), v.clone());
            }
        }
        let line = match fmt {
            Some(f) => format_map(f, &ctx),
            None => format!("[{i}] {ann}"),
        };
        lines.push(line);
    }
    lines.join("\n")
}
