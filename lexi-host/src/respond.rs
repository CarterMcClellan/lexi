//! The text-chat pipeline: compose (lexi-core) → POST → extract (lexi-core),
//! in both buffered and streaming (SSE) forms.

use futures_util::StreamExt;
use serde_json::{json, Map, Value};

use crate::{auth, config, HostError};

/// The result of a text chat: the extracted assistant text + the raw response
/// (the raw is `null` for streaming).
#[derive(Debug, Clone)]
pub struct RespondResult {
    pub text: String,
    pub raw: Value,
}

fn endpoint_url(effective_cfg: &Value) -> String {
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
}

/// Build the request body for a text prompt via the engine.
fn compose_body(
    effective_cfg: &Value,
    prompt: &str,
    role: &str,
    model: &str,
    stream: bool,
) -> Result<Value, HostError> {
    let mut payload = Map::new();
    payload.insert("prompt".into(), Value::from(prompt));
    payload.insert("role".into(), Value::from(role));
    payload.insert("model".into(), Value::from(model));
    let payload = Value::Object(payload);

    let msg = lexi_core::normalize_prompt_payload(&payload);
    let bindings = lexi_core::lower_message(&msg, effective_cfg.get("content_mapping"));
    let effective_payload = config::deep_merge(&payload, &bindings);

    let overrides = if stream {
        json!({ "model": model, "stream": true })
    } else {
        json!({ "model": model })
    };

    lexi_core::compose_request_body(
        effective_cfg,
        &effective_payload,
        &overrides,
        &[],
        &[],
        &[],
        false,
    )
    .map_err(HostError::Compose)
}

/// Buffered (non-streaming) pipeline over an already-resolved effective config.
/// No file/config I/O, so it is directly testable against a mock server.
pub async fn respond_with_effective(
    client: &reqwest::Client,
    effective_cfg: &Value,
    api_key: &str,
    prompt: &str,
    role: &str,
    model: &str,
) -> Result<RespondResult, HostError> {
    let body = compose_body(effective_cfg, prompt, role, model, false)?;
    let headers = auth::build_headers(effective_cfg, api_key);
    let resp = client
        .post(endpoint_url(effective_cfg))
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
    let text_extract = effective_cfg
        .get("response")
        .and_then(|r| r.get("text_extract"));
    let text = lexi_core::extract_text(&raw, text_extract);
    Ok(RespondResult { text, raw })
}

/// Streaming (SSE) pipeline: invokes `on_delta` with each text fragment as it
/// arrives and returns the accumulated text. Reads `response.stream` to classify
/// events via the engine.
pub async fn respond_stream_with_effective<F: FnMut(&str)>(
    client: &reqwest::Client,
    effective_cfg: &Value,
    api_key: &str,
    prompt: &str,
    role: &str,
    model: &str,
    mut on_delta: F,
) -> Result<RespondResult, HostError> {
    let body = compose_body(effective_cfg, prompt, role, model, true)?;
    let headers = auth::build_headers(effective_cfg, api_key);
    let resp = client
        .post(endpoint_url(effective_cfg))
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

    let stream_cfg = effective_cfg
        .get("response")
        .and_then(|r| r.get("stream"))
        .cloned()
        .unwrap_or(Value::Null);

    let mut buffer = String::new();
    let mut accumulated = String::new();
    let mut bytes = resp.bytes_stream();

    'outer: while let Some(chunk) = bytes.next().await {
        buffer.push_str(&String::from_utf8_lossy(&chunk?));
        // An SSE event block ends at a blank line.
        while let Some(idx) = buffer.find("\n\n") {
            let block: String = buffer.drain(..idx + 2).collect();
            let parsed = lexi_core::parse_sse_block(&block);
            let event = parsed["event"].as_str();
            let data = parsed["data"].as_str().unwrap_or("");
            let cls = lexi_core::classify_event(event, data, &stream_cfg);
            match cls["kind"].as_str() {
                Some("delta") => {
                    let frag = cls["text"].as_str().unwrap_or("");
                    accumulated.push_str(frag);
                    on_delta(frag);
                }
                Some("done") => break 'outer,
                _ => {}
            }
        }
    }

    Ok(RespondResult {
        text: accumulated,
        raw: Value::Null,
    })
}

async fn resolve(
    provider: &str,
    model_override: Option<&str>,
) -> Result<(Value, String, String), HostError> {
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
    Ok((eff, api_key, model))
}

/// Load config, resolve the provider, and run a buffered text chat.
pub async fn respond_text(
    provider: &str,
    prompt: &str,
    role: &str,
    model_override: Option<&str>,
) -> Result<RespondResult, HostError> {
    let (eff, api_key, model) = resolve(provider, model_override).await?;
    let client = reqwest::Client::new();
    respond_with_effective(&client, &eff, &api_key, prompt, role, &model).await
}

/// Load config, resolve the provider, and run a streaming text chat.
pub async fn respond_stream_text<F: FnMut(&str)>(
    provider: &str,
    prompt: &str,
    role: &str,
    model_override: Option<&str>,
    on_delta: F,
) -> Result<RespondResult, HostError> {
    let (eff, api_key, model) = resolve(provider, model_override).await?;
    let client = reqwest::Client::new();
    respond_stream_with_effective(&client, &eff, &api_key, prompt, role, &model, on_delta).await
}
