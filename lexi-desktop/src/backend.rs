//! `/api/*` request handling for the reused `lexi-gui` frontend.
//!
//! The frontend still speaks the Node server's HTTP contract; a `fetch`-shim in
//! the webview routes every `/api/*` call to the `api` Tauri command, which
//! dispatches here. Shapes match the original Node handlers. Chat (`providers`,
//! buffered + streaming `respond`) is real over `lexi-host`; management endpoints
//! are benign stubs for now (the admin tabs degrade gracefully).

use serde_json::{json, Map, Value};

use lexi_host::config;

/// Dispatch a shimmed `/api/*` request to a response value.
pub async fn handle(method: &str, path: &str, body: Option<Value>) -> Result<Value, String> {
    let base = path.split('?').next().unwrap_or(path);
    match (method, base) {
        ("GET", "/api/providers") | ("GET", "/api/providers/") => providers(),
        ("GET", "/api/config") | ("GET", "/api/config/") => Ok(json!({})),
        ("GET", "/api/prompts") => Ok(json!({ "prompts": [] })),
        ("GET", p) if p.starts_with("/api/models") => Ok(json!({ "models": [] })),
        ("GET", p) if p.starts_with("/api/respond/options") => Ok(json!({ "presets": [] })),
        ("POST", "/api/respond") => respond(body.unwrap_or(Value::Null)).await,
        _ => Err(format!(
            "endpoint not available in the desktop app: {method} {base}"
        )),
    }
}

/// `GET /api/providers` — the normalized provider list the chat tab renders.
fn providers() -> Result<Value, String> {
    let cfg = config::load_providers().map_err(|e| e.to_string())?;
    let home = config::lexi_home();
    let empty = Map::new();
    let providers = cfg
        .get("providers")
        .and_then(Value::as_object)
        .unwrap_or(&empty);

    let mut out = Vec::new();
    for (name, meta) in providers {
        let url = meta.get("url").and_then(Value::as_str).unwrap_or("");
        let default_model = meta
            .get("default_model")
            .and_then(Value::as_str)
            .unwrap_or("");
        let api = meta.get("api").and_then(Value::as_str).unwrap_or("");
        let has_key_meta = meta
            .get("api_key")
            .and_then(Value::as_str)
            .map(|s| !s.is_empty())
            .unwrap_or(false);
        let has_key = has_key_meta || home.join("providers").join(name).join("api-key").exists();
        let configured = !url.is_empty() && has_key && !default_model.is_empty();

        out.push(json!({
            "name": name,
            "url": url,
            "hosted_models": meta.get("hosted_models").cloned().unwrap_or(json!("")),
            "hosted_model_fields": meta.get("hosted_model_fields").cloned().unwrap_or(json!([])),
            "default_model": default_model,
            "api": api,
            "api_key": if has_key { "[set]" } else { "[missing]" },
            "configured": configured,
            "source": "providers-config.yaml",
        }));
    }
    Ok(json!({ "providers": out, "source": "providers-config.yaml" }))
}

/// Sentinel the webview shim watches for to close the ReadableStream.
pub const STREAM_END: &str = "__LEXI_STREAM_END__";

fn sse(event: &str, data: Value) -> String {
    format!("event: {event}\ndata: {data}\n\n")
}

/// `POST /api/respond/stream` — streaming chat. Emits the frontend's SSE protocol
/// (`provider_start` / `delta` / `provider_done`, keyed by provider) through the
/// Tauri channel, provider by provider, then a close sentinel.
pub async fn stream(body: Value, on_event: tauri::ipc::Channel<String>) -> Result<(), String> {
    let prompt = body.get("prompt").and_then(Value::as_str).unwrap_or("");
    let providers = body
        .get("providers")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let empty = Map::new();
    let models = body
        .get("models")
        .and_then(Value::as_object)
        .unwrap_or(&empty);

    for pv in &providers {
        let Some(provider) = pv.as_str() else {
            continue;
        };
        let provider = provider.to_string();
        let model = models
            .get(&provider)
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty());

        let _ = on_event.send(sse("provider_start", json!({ "provider": provider })));

        let ch = on_event.clone();
        let p = provider.clone();
        let result =
            lexi_host::respond_stream_text(&provider, prompt, "user", model, &[], move |frag| {
                let _ = ch.send(sse("delta", json!({ "provider": p, "text": frag })));
            })
            .await;

        let done = match result {
            Ok(_) => json!({ "provider": provider }),
            Err(e) => json!({ "provider": provider, "error": e.to_string() }),
        };
        let _ = on_event.send(sse("provider_done", done));
    }

    let _ = on_event.send(STREAM_END.to_string());
    Ok(())
}

/// `POST /api/respond` — buffered chat, fanned out over the selected providers.
async fn respond(body: Value) -> Result<Value, String> {
    let prompt = body.get("prompt").and_then(Value::as_str).unwrap_or("");
    if prompt.trim().is_empty() {
        return Err("prompt is required".into());
    }
    let providers = body
        .get("providers")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if providers.is_empty() {
        return Err("at least one provider must be selected".into());
    }
    let empty = Map::new();
    let models = body
        .get("models")
        .and_then(Value::as_object)
        .unwrap_or(&empty);

    let mut responses = Vec::new();
    for pv in &providers {
        let Some(provider) = pv.as_str() else {
            continue;
        };
        let model = models
            .get(provider)
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty());
        match lexi_host::respond_text(provider, prompt, "user", model, &[]).await {
            Ok(r) => {
                responses.push(json!({ "provider": provider, "output": r.text, "stderr": "" }))
            }
            Err(e) => responses.push(json!({ "provider": provider, "error": e.to_string() })),
        }
    }
    Ok(json!({ "responses": responses }))
}
