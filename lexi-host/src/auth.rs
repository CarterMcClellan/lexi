//! Build HTTP auth headers from an effective config's `auth` block.

use reqwest::header::{HeaderMap, HeaderName, HeaderValue, ACCEPT, CONTENT_TYPE};
use serde_json::Value;

fn header_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Mirrors the Python CLI's `build_auth_headers`: Content-Type/Accept JSON, the
/// configured auth header with its value prefix, and any `extra_headers`.
pub fn build_headers(effective_cfg: &Value, api_key: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert(ACCEPT, HeaderValue::from_static("application/json"));

    if crate::chatgpt::enabled(effective_cfg) {
        if let Ok(value) = HeaderValue::from_str(&format!("Bearer {api_key}")) {
            headers.insert(reqwest::header::AUTHORIZATION, value);
        }
        return headers;
    }
    let auth = effective_cfg.get("auth");
    let header_name = auth
        .and_then(|a| a.get("header"))
        .and_then(Value::as_str)
        .unwrap_or("Authorization");
    let prefix = auth
        .and_then(|a| a.get("value_prefix"))
        .and_then(Value::as_str)
        .unwrap_or("Bearer ");

    if let (Ok(name), Ok(val)) = (
        HeaderName::from_bytes(header_name.as_bytes()),
        HeaderValue::from_str(&format!("{prefix}{api_key}")),
    ) {
        headers.insert(name, val);
    }

    if let Some(extra) = auth
        .and_then(|a| a.get("extra_headers"))
        .and_then(Value::as_object)
    {
        for (k, v) in extra {
            if let (Ok(name), Ok(val)) = (
                HeaderName::from_bytes(k.as_bytes()),
                HeaderValue::from_str(&header_string(v)),
            ) {
                headers.insert(name, val);
            }
        }
    }
    headers
}
