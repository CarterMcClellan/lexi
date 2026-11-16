//! Hosted-model discovery: a live GET to a provider's models endpoint.
//!
//! Mirrors the Python CLI's `fetch_hosted_models` + the Node server's response
//! normalization: resolve the endpoint from the effective config, send an
//! authenticated request, and flatten the JSON to a list of model entries.

use std::time::Duration;

use serde_json::Value;

use crate::{auth, config, HostError};

/// How long to wait for a provider's models endpoint (matches the CLI).
const HOSTED_TIMEOUT: Duration = Duration::from_secs(15);

/// Normalize a hosted-models response to a flat list (mirrors the Node handler):
/// an object's `data` or `models` array, else its values; a top-level array
/// as-is; anything else an empty list.
fn normalize_hosted_items(data: &Value) -> Vec<Value> {
    if let Some(obj) = data.as_object() {
        if let Some(arr) = obj.get("data").and_then(Value::as_array) {
            return arr.clone();
        }
        if let Some(arr) = obj.get("models").and_then(Value::as_array) {
            return arr.clone();
        }
        return obj.values().cloned().collect();
    }
    if let Some(arr) = data.as_array() {
        return arr.clone();
    }
    Vec::new()
}

/// Compute the models endpoint from an effective config: the base URL —
/// normalized to a `/v1` root if it embeds `/v1/...` — plus the `hosted_models`
/// path (default `/models`).
fn hosted_endpoint(effective_cfg: &Value) -> String {
    let mut base = effective_cfg
        .get("url")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim_end_matches('/')
        .to_string();
    if let Some(idx) = base.find("/v1/") {
        base = format!("{}/v1", &base[..idx]);
    } else if let Some(stripped) = base.strip_suffix("/v1/messages") {
        base = format!("{stripped}/v1");
    }
    let path = effective_cfg
        .get("hosted_models")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .unwrap_or("/models");
    if path.starts_with('/') {
        format!("{base}{path}")
    } else {
        format!("{base}/{path}")
    }
}

/// Fetch and normalize a provider's hosted models over an already-resolved
/// effective config. No file/config I/O, so it is testable against a mock server.
pub async fn fetch_hosted_models_with(
    client: &reqwest::Client,
    effective_cfg: &Value,
    api_key: &str,
) -> Result<Vec<Value>, HostError> {
    let endpoint = hosted_endpoint(effective_cfg);
    let headers = auth::build_headers(effective_cfg, api_key);
    let resp = client
        .get(&endpoint)
        .headers(headers)
        .timeout(HOSTED_TIMEOUT)
        .send()
        .await?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(HostError::HttpStatus {
            status: status.as_u16(),
            body,
        });
    }
    let data: Value = resp.json().await?;
    Ok(normalize_hosted_items(&data))
}

/// Load config, resolve the provider, and fetch its hosted models.
pub async fn fetch_hosted_models(provider: &str) -> Result<Vec<Value>, HostError> {
    let providers = config::load_providers()?;
    let provider_cfg = providers
        .get("providers")
        .and_then(|p| p.get(provider))
        .cloned()
        .ok_or_else(|| HostError::UnknownProvider(provider.to_string()))?;
    let eff = config::effective_config(provider, &provider_cfg)?;
    let api_key = config::resolve_api_key(provider, &provider_cfg)?;
    let client = reqwest::Client::new();
    fetch_hosted_models_with(&client, &eff, &api_key).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn normalize_handles_data_models_values_and_array() {
        assert_eq!(
            normalize_hosted_items(&json!({ "data": [{ "id": "a" }] })).len(),
            1
        );
        assert_eq!(
            normalize_hosted_items(&json!({ "models": [{ "id": "a" }, { "id": "b" }] })).len(),
            2
        );
        assert_eq!(normalize_hosted_items(&json!([{ "id": "a" }])).len(), 1);
        assert!(normalize_hosted_items(&json!("nope")).is_empty());
    }

    #[test]
    fn endpoint_normalizes_v1_base() {
        let cfg = json!({ "url": "https://x.test/v1/messages", "hosted_models": "/models" });
        assert_eq!(hosted_endpoint(&cfg), "https://x.test/v1/models");

        let cfg = json!({ "url": "https://x.test/v1/chat/completions" });
        assert_eq!(hosted_endpoint(&cfg), "https://x.test/v1/models");

        let cfg = json!({ "url": "https://x.test", "hosted_models": "models" });
        assert_eq!(hosted_endpoint(&cfg), "https://x.test/models");
    }
}

/// A provider's API-key readiness, as `providers status` reports it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct KeyStatus {
    /// `"ready"`, `"no api-key"`, or `"invalid api-key"`.
    pub status: &'static str,
    /// Where the key comes from (see [`config::api_key_source`]).
    pub source: Option<&'static str>,
}

/// How long `check_api_key` waits for the models endpoint.
const KEY_CHECK_TIMEOUT: Duration = Duration::from_secs(5);

/// Check a provider's API key with a live authenticated GET to its models
/// endpoint: 401/403 means `invalid api-key`; success — or any other outcome
/// that can't prove the key bad (timeouts, 5xx, no endpoint) — means `ready`.
pub async fn check_api_key(provider: &str) -> Result<KeyStatus, HostError> {
    let providers = config::load_providers()?;
    let provider_cfg = providers
        .get("providers")
        .and_then(|p| p.get(provider))
        .cloned()
        .ok_or_else(|| HostError::UnknownProvider(provider.to_string()))?;
    let source = config::api_key_source(provider, &provider_cfg)?;
    let Some(source) = source else {
        return Ok(KeyStatus {
            status: "no api-key",
            source: None,
        });
    };
    let api_key = config::resolve_api_key(provider, &provider_cfg)?;
    let eff = config::effective_config(provider, &provider_cfg)?;
    let client = reqwest::Client::builder()
        .timeout(KEY_CHECK_TIMEOUT)
        .build()?;
    let status = match fetch_hosted_models_with(&client, &eff, &api_key).await {
        Err(HostError::HttpStatus {
            status: 401 | 403, ..
        }) => "invalid api-key",
        _ => "ready",
    };
    Ok(KeyStatus {
        status,
        source: Some(source),
    })
}
