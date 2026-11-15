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
        ("GET", "/api/config") | ("GET", "/api/config/") => Ok(config_get()),
        ("GET", "/api/prompts") => prompts_list(),
        ("GET", p) if p.starts_with("/api/models") => models(path),
        ("GET", p) if p.starts_with("/api/definitions/raw/") => api_raw(base),
        ("GET", "/api/definitions") | ("GET", "/api/definitions/") => api_list(),
        ("GET", p) if p.starts_with("/api/definitions/validate/") => Err(format!(
            "endpoint not available in the desktop app: {method} {base}"
        )),
        ("GET", p) if p.starts_with("/api/definitions/") => api_show(base),
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

/// `GET /api/models?provider=X` — a provider's configured models, enriched with
/// its default flag and aliases. Mirrors the Node `{ provider, models: [...] }`
/// shape; the chat tab reads `id`/`name`, the Models tab the rest.
fn models(path: &str) -> Result<Value, String> {
    let provider = query_param(path, "provider")
        .ok_or_else(|| "provider query param is required".to_string())?;

    let models = config::load_models(&provider).map_err(|e| e.to_string())?;
    let models: Vec<Value> = models
        .into_iter()
        .map(|m| {
            json!({
                "id": m.id,
                "description": m.description,
                "default": m.default,
                "aliases": m.aliases,
            })
        })
        .collect();
    Ok(json!({ "provider": provider, "models": models }))
}

/// `GET /api/config` — the CLI app settings, the saved `config.json` layered
/// over built-in defaults. Never errors (defaults stand in on any read failure).
fn config_get() -> Value {
    let mut cfg = json!({
        "cli_name": "lexi",
        "prompt": "lexi",
        "prompt_delimiter": ">",
        "edit_mode": "emacs",
    });
    if let Ok(Value::Object(saved)) = config::load_config() {
        let base = cfg.as_object_mut().unwrap();
        for (k, v) in saved {
            base.insert(k, v);
        }
    }
    cfg
}

/// `GET /api/prompts` — the saved prompt library. Flattens each prompt's body
/// into `{ name, ...body, isActive }`; the `$$` prompt is the active one.
fn prompts_list() -> Result<Value, String> {
    let prompts = config::load_prompts().map_err(|e| e.to_string())?;
    let prompts: Vec<Value> = prompts
        .into_iter()
        .map(|(name, body)| {
            let mut obj = Map::new();
            obj.insert("name".into(), json!(name));
            if let Value::Object(b) = body {
                for (k, v) in b {
                    obj.insert(k, v);
                }
            }
            obj.insert("isActive".into(), json!(name == "$$"));
            Value::Object(obj)
        })
        .collect();
    Ok(json!({ "prompts": prompts }))
}

/// `GET /api/definitions` — the available API definitions as `{ name, description }`.
fn api_list() -> Result<Value, String> {
    let defs = config::list_api_defs().map_err(|e| e.to_string())?;
    let apis: Vec<Value> = defs
        .into_iter()
        .map(|(name, description)| json!({ "name": name, "description": description }))
        .collect();
    Ok(json!({ "apis": apis }))
}

/// `GET /api/definitions/{name}` — a single definition, parsed.
fn api_show(base: &str) -> Result<Value, String> {
    let name = def_name(base, "/api/definitions/");
    let definition = config::load_api_def(&name).map_err(|e| e.to_string())?;
    Ok(json!({ "name": name, "definition": definition }))
}

/// `GET /api/definitions/raw/{name}` — a definition's raw YAML text (for the editor).
fn api_raw(base: &str) -> Result<Value, String> {
    let name = def_name(base, "/api/definitions/raw/");
    let yaml = config::load_api_raw(&name).map_err(|e| e.to_string())?;
    Ok(json!({ "name": name, "yaml": yaml }))
}

/// Extract (and percent-decode) a definition name from a `/api/definitions/...` path.
fn def_name(base: &str, prefix: &str) -> String {
    let rest = base
        .strip_prefix(prefix)
        .unwrap_or("")
        .trim_end_matches('/');
    percent_decode(rest)
}

/// Extract a single query-string parameter from a shimmed request path.
fn query_param(path: &str, key: &str) -> Option<String> {
    let query = path.split('?').nth(1)?;
    query.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        (k == key).then(|| percent_decode(v))
    })
}

/// Minimal percent-decoding for query values (`%XX` and `+` → space).
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
                match hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                    Some(b) => {
                        out.push(b);
                        i += 3;
                    }
                    None => {
                        out.push(bytes[i]);
                        i += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
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

#[cfg(test)]
mod tests {
    use super::{percent_decode, query_param};

    #[test]
    fn query_param_extracts_and_decodes() {
        assert_eq!(
            query_param("/api/models?provider=openai", "provider").as_deref(),
            Some("openai")
        );
        assert_eq!(
            query_param("/api/models?x=1&provider=a%2Fb", "provider").as_deref(),
            Some("a/b")
        );
        assert_eq!(query_param("/api/models", "provider"), None);
        assert_eq!(query_param("/api/models?other=1", "provider"), None);
    }

    #[test]
    fn percent_decode_handles_escapes_and_plus() {
        assert_eq!(percent_decode("meta%2Fllama+3"), "meta/llama 3");
        assert_eq!(percent_decode("plain"), "plain");
    }

    #[test]
    fn def_name_strips_prefix_and_decodes() {
        use super::def_name;
        assert_eq!(
            def_name("/api/definitions/messages", "/api/definitions/"),
            "messages"
        );
        assert_eq!(
            def_name("/api/definitions/raw/chat/", "/api/definitions/raw/"),
            "chat"
        );
        assert_eq!(
            def_name("/api/definitions/a%20b", "/api/definitions/"),
            "a b"
        );
    }
}
