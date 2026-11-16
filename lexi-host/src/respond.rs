//! The chat pipeline: compose (lexi-core) → POST → extract (lexi-core), in
//! buffered, streaming (SSE) and background (submit/poll) forms.
//!
//! Two layers:
//! - **Full chat** ([`ChatRequest`] → [`prepare_chat`] → [`run_chat`]): a saved
//!   prompt payload plus everything the CLI's `respond` takes — model (aliases
//!   resolved), images, `--params` overrides, tool presets, `--set`/`--unset`
//!   paths. Model resolution and per-model config overrides follow the Python
//!   CLI. `prepare_chat` does no network I/O, so the composed body is
//!   inspectable.
//! - **Simple text chat** (`respond_text` / `respond_stream_text` /
//!   `respond_background_text`, and their `*_with_effective` forms): prompt +
//!   role, used by the desktop app and the mock-server tests.

use futures_util::StreamExt;
use serde_json::{json, Map, Value};

use crate::image::ResolvedImage;
use crate::{auth, config, HostError};

/// The result of a chat: the extracted assistant text + the raw response.
/// For streaming, `raw` is the stream's final response event (`final_event`,
/// else `done_event`) when the API sends one, otherwise `null`.
#[derive(Debug, Clone)]
pub struct RespondResult {
    pub text: String,
    pub raw: Value,
}

/// A streaming callback event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamEvent<'a> {
    /// A fragment of assistant text.
    Delta(&'a str),
    /// A status label for provider-side work (e.g. "searching the web…"),
    /// from the stream config's `activity_events`.
    Activity(&'a str),
}

pub(crate) fn endpoint_url(effective_cfg: &Value) -> String {
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

/// Compose a request body from a prompt payload (`{prompt, role, max_tokens,
/// temperature, …}`) via the engine: the payload's text (plus any images)
/// becomes a canonical message lowered through the API's `content_mapping`;
/// the payload's other fields stay available as bindings. Precedence (low →
/// high): schema default → payload → `overrides` → tool presets → `set_paths`
/// → `unset_paths`.
#[allow(clippy::too_many_arguments)]
pub fn compose_chat_body(
    effective_cfg: &Value,
    payload: &Value,
    images: &[ResolvedImage],
    overrides: &Value,
    tool_presets: &[String],
    set_paths: &[(String, Value)],
    unset_paths: &[String],
    skip_validation: bool,
) -> Result<Value, HostError> {
    let text = payload.get("prompt").and_then(Value::as_str).unwrap_or("");
    let role = payload
        .get("role")
        .and_then(Value::as_str)
        .unwrap_or("user");
    let mut content = vec![json!({ "type": "text", "text": text })];
    for img in images {
        content.push(img.part_value());
    }
    let message = json!({ "role": role, "content": content });
    let msg = lexi_core::message_from_dict(&message);
    let lowered = lexi_core::lower_message(&msg, effective_cfg.get("content_mapping"));

    let mut bindings = payload.as_object().cloned().unwrap_or_default();
    if let Value::Object(l) = lowered {
        bindings.extend(l);
    }
    lexi_core::compose_request_body(
        effective_cfg,
        &Value::Object(bindings),
        overrides,
        tool_presets,
        set_paths,
        unset_paths,
        skip_validation,
    )
    .map_err(HostError::Compose)
}

/// Simple-chat body: just `prompt` + `role` (+ images) and `overrides`.
pub(crate) fn compose_body(
    effective_cfg: &Value,
    prompt: &str,
    role: &str,
    images: &[ResolvedImage],
    overrides: &Value,
) -> Result<Value, HostError> {
    compose_chat_body(
        effective_cfg,
        &json!({ "prompt": prompt, "role": role }),
        images,
        overrides,
        &[],
        &[],
        &[],
        false,
    )
}

fn text_extract(effective_cfg: &Value) -> Option<&Value> {
    effective_cfg
        .get("response")
        .and_then(|r| r.get("text_extract"))
}

async fn check_status(resp: reqwest::Response) -> Result<reqwest::Response, HostError> {
    let status = resp.status();
    if status.is_success() {
        return Ok(resp);
    }
    let body = resp.text().await.unwrap_or_default();
    Err(HostError::HttpStatus {
        status: status.as_u16(),
        body,
    })
}

/// POST a composed body and extract the text from the JSON reply.
async fn post_buffered(
    client: &reqwest::Client,
    effective_cfg: &Value,
    api_key: &str,
    body: &Value,
) -> Result<RespondResult, HostError> {
    let resp = client
        .post(endpoint_url(effective_cfg))
        .headers(auth::build_headers(effective_cfg, api_key))
        .json(body)
        .send()
        .await?;
    let raw: Value = check_status(resp).await?.json().await?;
    let text = lexi_core::extract_text(&raw, text_extract(effective_cfg));
    Ok(RespondResult { text, raw })
}

/// POST a composed body and consume the SSE reply: deltas and activity labels
/// go to `on_event`; the final response event (if any) becomes `raw`.
async fn post_stream<F: FnMut(StreamEvent)>(
    client: &reqwest::Client,
    effective_cfg: &Value,
    api_key: &str,
    body: &Value,
    mut on_event: F,
) -> Result<RespondResult, HostError> {
    let resp = client
        .post(endpoint_url(effective_cfg))
        .headers(auth::build_headers(effective_cfg, api_key))
        .header(reqwest::header::ACCEPT, "text/event-stream")
        .json(body)
        .send()
        .await?;
    let resp = check_status(resp).await?;

    let stream_cfg = effective_cfg
        .get("response")
        .and_then(|r| r.get("stream"))
        .cloned()
        .unwrap_or(Value::Null);
    let final_event = stream_cfg
        .get("final_event")
        .or_else(|| stream_cfg.get("done_event"))
        .and_then(Value::as_str)
        .map(str::to_string);

    let mut buffer = String::new();
    let mut accumulated = String::new();
    let mut final_response = Value::Null;
    let mut bytes = resp.bytes_stream();

    'outer: while let Some(chunk) = bytes.next().await {
        buffer.push_str(&String::from_utf8_lossy(&chunk?));
        // An SSE event block ends at a blank line.
        while let Some(idx) = buffer.find("\n\n") {
            let block: String = buffer.drain(..idx + 2).collect();
            let parsed = lexi_core::parse_sse_block(&block);
            let event = parsed["event"].as_str();
            let data = parsed["data"].as_str().unwrap_or("");
            if final_event.is_some() && event == final_event.as_deref() {
                if let Ok(v @ Value::Object(_)) = serde_json::from_str::<Value>(data) {
                    final_response = v;
                }
            }
            let cls = lexi_core::classify_event(event, data, &stream_cfg);
            match cls["kind"].as_str() {
                Some("delta") => {
                    let frag = cls["text"].as_str().unwrap_or("");
                    accumulated.push_str(frag);
                    on_event(StreamEvent::Delta(frag));
                }
                Some("activity") => {
                    if let Some(label) = cls["label"].as_str() {
                        on_event(StreamEvent::Activity(label));
                    }
                }
                Some("done") => break 'outer,
                _ => {}
            }
        }
    }

    Ok(RespondResult {
        text: accumulated,
        raw: final_response,
    })
}

/// POST a composed background body, then poll `operation.poll` until a
/// terminal state and extract the result. The engine (`read_operation` /
/// `poll_path`) classifies each poll; the host does the HTTP and the sleeps.
async fn post_background(
    client: &reqwest::Client,
    effective_cfg: &Value,
    api_key: &str,
    body: &Value,
    poll_interval: std::time::Duration,
    max_polls: usize,
) -> Result<RespondResult, HostError> {
    let operation_cfg = effective_cfg.get("operation").cloned().ok_or_else(|| {
        HostError::Config("this API defines no `operation` (background) lifecycle".into())
    })?;
    let headers = auth::build_headers(effective_cfg, api_key);
    let base_url = effective_cfg
        .get("url")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();

    let resp = client
        .post(endpoint_url(effective_cfg))
        .headers(headers.clone())
        .json(body)
        .send()
        .await?;
    let mut current: Value = check_status(resp).await?.json().await?;
    let mut op = lexi_core::read_operation(&current, Some(&operation_cfg));

    let mut polls = 0;
    while !op["done"].as_bool().unwrap_or(false) {
        if polls >= max_polls {
            return Err(HostError::OperationTimeout);
        }
        tokio::time::sleep(poll_interval).await;
        let id = op.get("id").cloned().unwrap_or(Value::Null);
        let poll_url = format!("{base_url}{}", lexi_core::poll_path(&operation_cfg, &id));
        let presp = client
            .get(&poll_url)
            .headers(headers.clone())
            .send()
            .await?;
        current = check_status(presp).await?.json().await?;
        op = lexi_core::read_operation(&current, Some(&operation_cfg));
        polls += 1;
    }

    if op["state"].as_str() == Some("failed") {
        return Err(HostError::OperationFailed(
            op["status"].as_str().unwrap_or("unknown").to_string(),
        ));
    }
    let text = lexi_core::extract_text(&current, text_extract(effective_cfg));
    Ok(RespondResult { text, raw: current })
}

// ---------------------------------------------------------------------------
// Simple text chat over an already-resolved effective config (testable
// against a mock server; no config I/O).
// ---------------------------------------------------------------------------

/// Buffered text chat.
pub async fn respond_with_effective(
    client: &reqwest::Client,
    effective_cfg: &Value,
    api_key: &str,
    prompt: &str,
    role: &str,
    model: &str,
    images: &[ResolvedImage],
) -> Result<RespondResult, HostError> {
    let body = compose_body(
        effective_cfg,
        prompt,
        role,
        images,
        &json!({ "model": model }),
    )?;
    post_buffered(client, effective_cfg, api_key, &body).await
}

/// Streaming text chat: `on_delta` receives each text fragment as it arrives.
#[allow(clippy::too_many_arguments)]
pub async fn respond_stream_with_effective<F: FnMut(&str)>(
    client: &reqwest::Client,
    effective_cfg: &Value,
    api_key: &str,
    prompt: &str,
    role: &str,
    model: &str,
    images: &[ResolvedImage],
    mut on_delta: F,
) -> Result<RespondResult, HostError> {
    let body = compose_body(
        effective_cfg,
        prompt,
        role,
        images,
        &json!({ "model": model, "stream": true }),
    )?;
    post_stream(client, effective_cfg, api_key, &body, |ev| {
        if let StreamEvent::Delta(t) = ev {
            on_delta(t);
        }
    })
    .await
}

/// Background (submit/poll/fetch) text chat.
#[allow(clippy::too_many_arguments)]
pub async fn respond_background_with_effective(
    client: &reqwest::Client,
    effective_cfg: &Value,
    api_key: &str,
    prompt: &str,
    role: &str,
    model: &str,
    poll_interval: std::time::Duration,
    max_polls: usize,
) -> Result<RespondResult, HostError> {
    let body = compose_body(
        effective_cfg,
        prompt,
        role,
        &[],
        &json!({ "model": model, "background": true }),
    )?;
    post_background(
        client,
        effective_cfg,
        api_key,
        &body,
        poll_interval,
        max_polls,
    )
    .await
}

// ---------------------------------------------------------------------------
// Provider + model resolution (shared by both layers).
// ---------------------------------------------------------------------------

/// Resolve `provider` and its model, returning the effective config (with any
/// per-model `auth`/`request`/`response` overrides merged in), the API key, and
/// the model id. Model: an override is resolved through the provider's
/// aliases and must be configured (when the provider lists models); otherwise
/// `default-model.yaml`, else the config's `default_model`.
fn resolve_provider_model(
    provider: &str,
    model_override: Option<&str>,
) -> Result<(Value, String, String), HostError> {
    let providers = config::load_providers()?;
    let mut provider_cfg = providers
        .get("providers")
        .and_then(|p| p.get(provider))
        .cloned()
        .ok_or_else(|| HostError::UnknownProvider(provider.to_string()))?;
    let models = config::load_models(provider)?;

    let model = match model_override {
        Some(m) => {
            let aliases = config::model_aliases(provider)?;
            if let Some(target) = aliases.get(m).and_then(Value::as_str) {
                target.to_string()
            } else if !models.is_empty() && !models.iter().any(|x| x.id == m) {
                let mut msg = format!(
                    "model {m:?} is not a configured model or alias for provider {provider:?}"
                );
                if !aliases.is_empty() {
                    let mut names: Vec<&str> = aliases.keys().map(String::as_str).collect();
                    names.sort();
                    msg.push_str(&format!("\navailable aliases: {}", names.join(", ")));
                }
                let mut ids: Vec<&str> = models.iter().map(|x| x.id.as_str()).collect();
                ids.sort();
                msg.push_str(&format!("\nconfigured models: {}", ids.join(", ")));
                return Err(HostError::Config(msg));
            } else {
                m.to_string()
            }
        }
        None => config::default_model(provider)?
            .or_else(|| {
                provider_cfg
                    .get("default_model")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .ok_or_else(|| {
                HostError::Config("no model (pass --model or set a default model)".into())
            })?,
    };

    // Per-model overrides of the API's auth/request/response blocks.
    if let Some(meta) = models.iter().find(|x| x.id == model).map(|x| &x.metadata) {
        if let Some(cfg) = provider_cfg.as_object_mut() {
            for key in ["auth", "request", "response"] {
                if let Some(over) = meta.get(key) {
                    let merged = match cfg.get(key) {
                        Some(existing) if existing.is_object() && over.is_object() => {
                            config::deep_merge(existing, over)
                        }
                        _ => over.clone(),
                    };
                    cfg.insert(key.to_string(), merged);
                }
            }
        }
    }

    let eff = config::effective_config(provider, &provider_cfg)?;
    let api_key = config::resolve_api_key(provider, &provider_cfg)?;
    Ok((eff, api_key, model))
}

// ---------------------------------------------------------------------------
// Simple text chat with config resolution (the desktop app's entry points).
// ---------------------------------------------------------------------------

/// Load config, resolve the provider, and run a buffered chat (optionally with
/// image parts).
pub async fn respond_text(
    provider: &str,
    prompt: &str,
    role: &str,
    model_override: Option<&str>,
    images: &[ResolvedImage],
) -> Result<RespondResult, HostError> {
    let (eff, api_key, model) = resolve_provider_model(provider, model_override)?;
    let client = reqwest::Client::new();
    respond_with_effective(&client, &eff, &api_key, prompt, role, &model, images).await
}

/// Load config, resolve the provider, and run a streaming chat (optionally with
/// image parts).
pub async fn respond_stream_text<F: FnMut(&str)>(
    provider: &str,
    prompt: &str,
    role: &str,
    model_override: Option<&str>,
    images: &[ResolvedImage],
    on_delta: F,
) -> Result<RespondResult, HostError> {
    let (eff, api_key, model) = resolve_provider_model(provider, model_override)?;
    let client = reqwest::Client::new();
    respond_stream_with_effective(
        &client, &eff, &api_key, prompt, role, &model, images, on_delta,
    )
    .await
}

/// Load config, resolve the provider, and run a background (submit/poll/fetch)
/// text chat. Polls every second for up to ~10 minutes.
pub async fn respond_background_text(
    provider: &str,
    prompt: &str,
    role: &str,
    model_override: Option<&str>,
) -> Result<RespondResult, HostError> {
    let (eff, api_key, model) = resolve_provider_model(provider, model_override)?;
    let client = reqwest::Client::new();
    respond_background_with_effective(
        &client,
        &eff,
        &api_key,
        prompt,
        role,
        &model,
        std::time::Duration::from_secs(1),
        600,
    )
    .await
}

// ---------------------------------------------------------------------------
// Full chat (the CLI's `respond`).
// ---------------------------------------------------------------------------

/// Everything a full chat can carry beyond the provider name.
#[derive(Debug, Clone, Default)]
pub struct ChatRequest {
    /// The prompt payload: `{prompt, role?, max_tokens?, temperature?, …}`.
    pub prompt: Value,
    /// Model override (an alias or configured id).
    pub model: Option<String>,
    pub images: Vec<ResolvedImage>,
    /// Body overrides (e.g. from a `--params` file).
    pub overrides: Map<String, Value>,
    pub tool_presets: Vec<String>,
    pub set_paths: Vec<(String, Value)>,
    pub unset_paths: Vec<String>,
    /// Compose without validating the body against the schema.
    pub skip_validation: bool,
}

/// How to run a chat. The composed body decides in the end: a body with
/// `stream: true` (e.g. via `--set stream=true`) streams regardless.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatMode {
    Buffered,
    Stream,
    Background,
}

/// A resolved, composed chat ready to send.
#[derive(Debug, Clone)]
pub struct PreparedChat {
    pub provider: String,
    pub model: String,
    pub effective: Value,
    pub body: Value,
    api_key: String,
}

impl PreparedChat {
    /// Whether [`run_chat`] will stream (the body asks for `stream: true`).
    pub fn streams(&self) -> bool {
        self.body.get("stream") == Some(&Value::Bool(true))
    }
}

/// Resolve the provider and model and compose the request body. No network I/O.
pub fn prepare_chat(
    provider: &str,
    req: &ChatRequest,
    mode: ChatMode,
) -> Result<PreparedChat, HostError> {
    let text = req
        .prompt
        .get("prompt")
        .and_then(Value::as_str)
        .unwrap_or("");
    if text.trim().is_empty() {
        return Err(HostError::Config("prompt text is empty".into()));
    }
    let (eff, api_key, model) = resolve_provider_model(provider, req.model.as_deref())?;
    let mut overrides = req.overrides.clone();
    overrides
        .entry("model")
        .or_insert_with(|| Value::from(model.clone()));
    match mode {
        ChatMode::Buffered => {}
        ChatMode::Stream => {
            overrides.insert("stream".into(), Value::Bool(true));
        }
        ChatMode::Background => {
            overrides.insert("background".into(), Value::Bool(true));
        }
    }
    let body = compose_chat_body(
        &eff,
        &req.prompt,
        &req.images,
        &Value::Object(overrides),
        &req.tool_presets,
        &req.set_paths,
        &req.unset_paths,
        req.skip_validation,
    )?;
    Ok(PreparedChat {
        provider: provider.to_string(),
        model,
        effective: eff,
        body,
        api_key,
    })
}

/// Send a prepared chat. Streams when the body asks for it (events go to
/// `on_event`), runs the background lifecycle when it asks for that, and is
/// buffered otherwise.
pub async fn run_chat<F: FnMut(StreamEvent)>(
    chat: &PreparedChat,
    on_event: F,
) -> Result<RespondResult, HostError> {
    let client = reqwest::Client::new();
    if chat.streams() {
        post_stream(
            &client,
            &chat.effective,
            &chat.api_key,
            &chat.body,
            on_event,
        )
        .await
    } else if chat.body.get("background") == Some(&Value::Bool(true)) {
        post_background(
            &client,
            &chat.effective,
            &chat.api_key,
            &chat.body,
            std::time::Duration::from_secs(1),
            600,
        )
        .await
    } else {
        post_buffered(&client, &chat.effective, &chat.api_key, &chat.body).await
    }
}
