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
        ("GET", p) if p.starts_with("/api/models/default") => models_default_get(path),
        ("GET", p) if p.starts_with("/api/models/aliases") => models_aliases_get(path),
        ("GET", p) if p.starts_with("/api/models/hosted") => models_hosted(path).await,
        ("GET", p) if p.starts_with("/api/models") => models(path),
        ("GET", p) if p.starts_with("/api/definitions/raw/") => api_raw(base),
        ("GET", "/api/definitions") | ("GET", "/api/definitions/") => api_list(),
        ("GET", p) if p.starts_with("/api/definitions/validate/") => api_validate(base),
        ("GET", p) if p.starts_with("/api/definitions/") => api_show(base),
        ("GET", p) if p.starts_with("/api/respond/options") => Ok(json!({ "presets": [] })),
        ("POST", "/api/config") | ("POST", "/api/config/") => {
            config_set(body.unwrap_or(Value::Null))
        }
        ("POST", "/api/prompts") => prompt_save(body.unwrap_or(Value::Null)),
        ("POST", "/api/prompts/active") => prompt_active(body.unwrap_or(Value::Null)),
        ("POST", "/api/prompts/delete") => prompt_delete(body.unwrap_or(Value::Null)),
        ("POST", "/api/models/default") => models_set_default(body.unwrap_or(Value::Null)),
        ("POST", "/api/models/aliases") => models_set_alias(body.unwrap_or(Value::Null)),
        ("POST", "/api/models/aliases/delete") => models_delete_alias(body.unwrap_or(Value::Null)),
        ("POST", "/api/models/add") => models_upsert(body.unwrap_or(Value::Null), false),
        ("POST", "/api/models/set") => models_upsert(body.unwrap_or(Value::Null), true),
        ("POST", "/api/models/delete") => models_delete(body.unwrap_or(Value::Null)),
        ("POST", "/api/definitions/create") => api_create(body.unwrap_or(Value::Null)),
        ("POST", "/api/definitions/save-raw") => api_save_raw(body.unwrap_or(Value::Null)),
        ("POST", "/api/definitions/delete") => api_delete(body.unwrap_or(Value::Null)),
        ("POST", "/api/definitions/preview") => api_preview(body.unwrap_or(Value::Null)),
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

/// `GET /api/models/default?provider=X` — the provider's current default model.
fn models_default_get(path: &str) -> Result<Value, String> {
    let provider = require_provider(path)?;
    let default = config::default_model(&provider).map_err(|e| e.to_string())?;
    Ok(json!({ "provider": provider, "default_model": default }))
}

/// `GET /api/models/aliases?provider=X` — the provider's `alias -> model` map.
fn models_aliases_get(path: &str) -> Result<Value, String> {
    let provider = require_provider(path)?;
    let aliases = config::model_aliases(&provider).map_err(|e| e.to_string())?;
    Ok(json!({ "provider": provider, "aliases": aliases }))
}

/// `GET /api/models/hosted?provider=X` — models the provider reports via its API
/// (a live call to the provider's models endpoint).
async fn models_hosted(path: &str) -> Result<Value, String> {
    let provider = require_provider(path)?;
    let models = lexi_host::fetch_hosted_models(&provider)
        .await
        .map_err(|e| e.to_string())?;
    Ok(json!({ "provider": provider, "models": models }))
}

/// `POST /api/models/default` — set the provider's default model.
fn models_set_default(body: Value) -> Result<Value, String> {
    let provider = require_field(&body, "provider")?;
    let model = require_field(&body, "model")?;
    config::set_default_model(&provider, Some(&model)).map_err(|e| e.to_string())?;
    Ok(json!({ "provider": provider, "default_model": model, "output": "" }))
}

/// `POST /api/models/aliases` — point an alias at a model.
fn models_set_alias(body: Value) -> Result<Value, String> {
    let provider = require_field(&body, "provider")?;
    let alias = require_field(&body, "alias")?;
    let model = require_field(&body, "model")?;
    config::set_model_alias(&provider, &alias, &model).map_err(|e| e.to_string())?;
    Ok(json!({ "provider": provider, "alias": alias, "model": model, "output": "" }))
}

/// `POST /api/models/aliases/delete` — remove an alias.
fn models_delete_alias(body: Value) -> Result<Value, String> {
    let provider = require_field(&body, "provider")?;
    let alias = require_field(&body, "alias")?;
    config::delete_model_alias(&provider, &alias).map_err(|e| e.to_string())?;
    Ok(json!({ "provider": provider, "alias": alias, "output": "" }))
}

/// `POST /api/models/add` and `/api/models/set` — add or update a model (both
/// upsert-merge). `is_set` only affects the reported action word.
fn models_upsert(body: Value, is_set: bool) -> Result<Value, String> {
    let provider = require_field(&body, "provider")?;
    let model = require_field(&body, "model")?;

    let mut meta = Map::new();
    for key in ["name", "description"] {
        if let Some(v) = body
            .get(key)
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
        {
            meta.insert(key.into(), json!(v));
        }
    }
    for key in ["max_tokens", "context_window"] {
        if let Some(v) = int_field(&body, key) {
            meta.insert(key.into(), v);
        }
    }
    let set_default = body
        .get("set_default")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let existed =
        config::upsert_model(&provider, &model, &meta, set_default).map_err(|e| e.to_string())?;
    let action = if is_set && existed {
        "Updated"
    } else {
        "Added"
    };
    Ok(json!({
        "provider": provider,
        "model": model,
        "output": format!("{action} model {model} for {provider}."),
    }))
}

/// `POST /api/models/delete` — remove a model (fixing the default if needed).
fn models_delete(body: Value) -> Result<Value, String> {
    let provider = require_field(&body, "provider")?;
    let model = require_field(&body, "model")?;
    config::delete_model(&provider, &model).map_err(|e| e.to_string())?;
    Ok(json!({ "provider": provider, "model": model, "output": "" }))
}

/// Require a `provider` query param from a request path.
fn require_provider(path: &str) -> Result<String, String> {
    query_param(path, "provider").ok_or_else(|| "provider query param is required".to_string())
}

/// Require a non-empty string field from a request body.
fn require_field(body: &Value, key: &str) -> Result<String, String> {
    body.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .ok_or_else(|| format!("{key} is required"))
}

/// Coerce an integer body field (number or numeric string), else `None`.
fn int_field(body: &Value, key: &str) -> Option<Value> {
    match body.get(key) {
        Some(Value::Number(n)) if n.is_i64() || n.is_u64() => Some(Value::Number(n.clone())),
        Some(Value::Number(n)) => n.as_f64().map(|f| json!(f as i64)),
        Some(Value::String(s)) => s.trim().parse::<i64>().ok().map(|i| json!(i)),
        _ => None,
    }
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

/// `POST /api/config` — merge the posted keys into `config.json`, then return the
/// updated settings (defaults layered on, as the GET does).
fn config_set(body: Value) -> Result<Value, String> {
    let updates = body
        .as_object()
        .ok_or_else(|| "config body must be an object".to_string())?;
    config::set_config(updates).map_err(|e| e.to_string())?;
    Ok(config_get())
}

/// `POST /api/prompts` — save a prompt (named, or the active `$$` slot when
/// unnamed), then return the refreshed prompt list.
fn prompt_save(body: Value) -> Result<Value, String> {
    let prompt = body
        .get("prompt")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Prompt text is required".to_string())?;

    let mut obj = Map::new();
    obj.insert("prompt".into(), json!(prompt));
    if let Some(v) = num_field(&body, "max_tokens") {
        obj.insert("max_tokens".into(), v);
    }
    if let Some(v) = num_field(&body, "temperature") {
        obj.insert("temperature".into(), v);
    }
    if let Some(role) = body
        .get("role")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
    {
        obj.insert("role".into(), json!(role));
    }

    let name = body
        .get("name")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .unwrap_or("$$");
    config::save_prompt(name, &Value::Object(obj)).map_err(|e| e.to_string())?;
    prompts_list()
}

/// `POST /api/prompts/active` — make the named prompt active, then refresh the list.
fn prompt_active(body: Value) -> Result<Value, String> {
    let name = require_name(&body)?;
    config::set_active_prompt(&name).map_err(|e| e.to_string())?;
    prompts_list()
}

/// `POST /api/prompts/delete` — delete the named prompt, then refresh the list.
fn prompt_delete(body: Value) -> Result<Value, String> {
    let name = require_name(&body)?;
    config::delete_prompt(&name).map_err(|e| e.to_string())?;
    prompts_list()
}

/// Pull a required `name` string from a request body.
fn require_name(body: &Value) -> Result<String, String> {
    body.get("name")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .ok_or_else(|| "Prompt name is required".to_string())
}

/// Coerce a numeric prompt field: keep numbers, parse numeric strings, else drop.
fn num_field(body: &Value, key: &str) -> Option<Value> {
    match body.get(key) {
        Some(Value::Number(n)) => Some(Value::Number(n.clone())),
        Some(Value::String(s)) => serde_json::from_str::<serde_json::Number>(s.trim())
            .ok()
            .map(Value::Number),
        _ => None,
    }
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

/// `GET /api/definitions/validate/{name}` — validate a stored definition.
fn api_validate(base: &str) -> Result<Value, String> {
    let name = def_name(base, "/api/definitions/validate/");
    let (valid, errors) = config::validate_api_def(&name).map_err(|e| e.to_string())?;
    Ok(json!({ "name": name, "valid": valid, "output": errors.join("\n") }))
}

/// `POST /api/definitions/create` — scaffold or clone a new definition.
fn api_create(body: Value) -> Result<Value, String> {
    let name = require_field(&body, "name")?;
    let from = body
        .get("from")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty());
    let path = body
        .get("path")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty());
    let text_path = body
        .get("text_path")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty());
    config::create_api_def(&name, from, path, text_path).map_err(|e| e.to_string())?;
    Ok(json!({ "name": name, "output": format!("Created {name:?}.") }))
}

/// `POST /api/definitions/save-raw` — save edited YAML, then validate.
fn api_save_raw(body: Value) -> Result<Value, String> {
    let name = require_field(&body, "name")?;
    let yaml = body
        .get("yaml")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "yaml content is required".to_string())?;
    let errors = config::save_api_raw(&name, yaml).map_err(|e| e.to_string())?;
    Ok(json!({
        "name": name,
        "valid": errors.is_empty(),
        "validation": errors.join("\n"),
    }))
}

/// `POST /api/definitions/delete` — remove a custom definition.
fn api_delete(body: Value) -> Result<Value, String> {
    let name = require_field(&body, "name")?;
    config::delete_api_def(&name).map_err(|e| e.to_string())?;
    Ok(json!({ "name": name, "output": format!("Removed {name:?}.") }))
}

/// `POST /api/definitions/preview` — live-preview a definition (from edited `yaml`
/// or a stored `name`) with sample inputs: validation, flags, presets, and a
/// sample composed body.
fn api_preview(body: Value) -> Result<Value, String> {
    let yaml = body
        .get("yaml")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty());
    let name = body
        .get("name")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty());
    let prompt = body
        .get("prompt")
        .and_then(Value::as_str)
        .unwrap_or("hello world");
    let model = body
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("sample-model");
    let role = body.get("role").and_then(Value::as_str).unwrap_or("user");
    config::preview_api_def(name, yaml, prompt, model, role).map_err(|e| e.to_string())
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

    #[test]
    fn num_field_coerces_numbers_and_numeric_strings() {
        use super::num_field;
        use serde_json::json;
        let body = json!({ "a": 256, "b": "0.7", "c": "nope", "d": true });
        assert_eq!(num_field(&body, "a"), Some(json!(256)));
        assert_eq!(num_field(&body, "b"), Some(json!(0.7)));
        assert_eq!(num_field(&body, "c"), None);
        assert_eq!(num_field(&body, "d"), None);
        assert_eq!(num_field(&body, "missing"), None);
    }

    #[test]
    fn int_field_coerces_to_integers() {
        use super::int_field;
        use serde_json::json;
        let body = json!({ "a": 4096, "b": "128000", "c": 1.9, "d": "x", "e": true });
        assert_eq!(int_field(&body, "a"), Some(json!(4096)));
        assert_eq!(int_field(&body, "b"), Some(json!(128000)));
        assert_eq!(int_field(&body, "c"), Some(json!(1))); // truncates floats
        assert_eq!(int_field(&body, "d"), None);
        assert_eq!(int_field(&body, "e"), None);
        assert_eq!(int_field(&body, "missing"), None);
    }
}
