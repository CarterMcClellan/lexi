//! The text-chat pipeline: compose (lexi-core) → POST → extract (lexi-core).

use serde_json::{json, Map, Value};

use crate::{auth, config, HostError};

/// The result of a text chat: the extracted assistant text + the raw response.
#[derive(Debug, Clone)]
pub struct RespondResult {
    pub text: String,
    pub raw: Value,
}

/// Core pipeline over an already-resolved effective config. No file/config I/O,
/// so it is directly testable against a mock server.
pub async fn respond_with_effective(
    client: &reqwest::Client,
    effective_cfg: &Value,
    api_key: &str,
    prompt: &str,
    role: &str,
    model: &str,
) -> Result<RespondResult, HostError> {
    let url = {
        let base = effective_cfg
            .get("url")
            .and_then(Value::as_str)
            .unwrap_or("");
        let path = effective_cfg
            .get("request")
            .and_then(|r| r.get("path"))
            .and_then(Value::as_str)
            .unwrap_or("");
        format!("{base}{path}")
    };

    // Build the request body via the engine.
    let mut payload = Map::new();
    payload.insert("prompt".into(), Value::from(prompt));
    payload.insert("role".into(), Value::from(role));
    payload.insert("model".into(), Value::from(model));
    let payload = Value::Object(payload);

    let msg = lexi_core::normalize_prompt_payload(&payload);
    let bindings = lexi_core::lower_message(&msg, effective_cfg.get("content_mapping"));
    let effective_payload = config::deep_merge(&payload, &bindings);
    let overrides = json!({ "model": model });

    let body = lexi_core::compose_request_body(
        effective_cfg,
        &effective_payload,
        &overrides,
        &[],
        &[],
        &[],
        false,
    )
    .map_err(HostError::Compose)?;

    // Transport.
    let headers = auth::build_headers(effective_cfg, api_key);
    let resp = client
        .post(&url)
        .headers(headers)
        .json(&body)
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
    let raw: Value = resp.json().await?;

    // Extract via the engine.
    let text_extract = effective_cfg
        .get("response")
        .and_then(|r| r.get("text_extract"));
    let text = lexi_core::extract_text(&raw, text_extract);
    Ok(RespondResult { text, raw })
}

/// Load config, resolve the provider, and run a text chat.
pub async fn respond_text(
    provider: &str,
    prompt: &str,
    role: &str,
    model_override: Option<&str>,
) -> Result<RespondResult, HostError> {
    let providers = config::load_providers()?;
    let provider_cfg = providers
        .get("providers")
        .and_then(|p| p.get(provider))
        .cloned()
        .ok_or_else(|| HostError::UnknownProvider(provider.to_string()))?;

    let eff = config::effective_config(provider, &provider_cfg)?;
    let api_key = config::resolve_api_key(provider, &provider_cfg)?;
    let model = model_override
        .map(String::from)
        .or_else(|| {
            eff.get("default_model")
                .and_then(Value::as_str)
                .map(String::from)
        })
        .ok_or_else(|| HostError::Config("no model (pass --model or set default_model)".into()))?;

    let client = reqwest::Client::new();
    respond_with_effective(&client, &eff, &api_key, prompt, role, &model).await
}
