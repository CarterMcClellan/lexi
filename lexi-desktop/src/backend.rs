//! `/api/*` request handling for the reused web frontend.
//!
//! The frontend still speaks the Node server's HTTP contract; a `fetch`-shim in
//! the webview routes every `/api/*` call to the `api` Tauri command, which
//! dispatches here. Shapes match the original Node handlers. Every endpoint is
//! served in-process by `lexi-host`: chat (buffered + streaming, fanned out over
//! providers concurrently) plus the full admin surface (providers, models,
//! prompts, config, API definitions) and image generation (the Images tab).
//! Only chat, image generation and hosted-model discovery call a provider; the
//! rest read/write `~/.lexi-cli/`.

use serde_json::{json, Map, Value};

use lexi_host::config;

/// Dispatch a shimmed `/api/*` request to a response value.
pub async fn handle(method: &str, path: &str, body: Option<Value>) -> Result<Value, String> {
    let base = path.split('?').next().unwrap_or(path);
    match (method, base) {
        ("GET", "/api/providers") | ("GET", "/api/providers/") => providers(),
        ("GET", "/api/config") | ("GET", "/api/config/") => Ok(config_get()),
        ("GET", "/api/themes") => Ok(themes()),
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
        ("GET", "/api/images/providers") => images_providers(),
        ("GET", "/api/images/models") => images_models(path).await,
        ("GET", "/api/images/options") => images_options(path),
        ("POST", "/api/images/generate") => images_generate(body.unwrap_or(Value::Null)).await,
        ("POST", "/api/images/reveal") => images_reveal(body.unwrap_or(Value::Null)),
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

    // The catch-all API and default model, read through templates and bindings.
    let summaries = lexi_host::admin::provider_summaries().map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for (name, meta) in providers {
        let summary = summaries.iter().find(|s| s["name"] == name.as_str());
        let field = |k: &str| {
            summary
                .and_then(|s| s.get(k))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string()
        };
        let url = meta.get("url").and_then(Value::as_str).unwrap_or("");
        let default_model = field("default_model");
        let api = field("api");
        let has_key_meta = meta
            .get("api_key")
            .and_then(Value::as_str)
            .map(|s| !s.is_empty())
            .unwrap_or(false);
        let has_key = has_key_meta || home.join("providers").join(name).join("api-key").exists();
        let configured = !url.is_empty() && has_key && !default_model.is_empty();
        // Whether it can chat: a binding that produces text, serving a chat
        // model by default. The Chat tab lists only these providers.
        let chat = lexi_host::resolve_model(name, None, Some("text"))
            .is_ok_and(|r| r.kind.as_deref().is_none_or(|k| k == "chat"));

        out.push(json!({
            "name": name,
            "url": url,
            "hosted_models": summary.and_then(|s| s.get("hosted_models")).filter(|v| !v.is_null()).cloned().unwrap_or(json!("")),
            "hosted_model_fields": summary.and_then(|s| s.get("hosted_model_fields")).filter(|v| !v.is_null()).cloned().unwrap_or(json!([])),
            "default_model": default_model,
            "api": api,
            "api_key": if has_key { "[set]" } else { "[missing]" },
            "configured": configured,
            "chat": chat,
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

    // Configured models with their classification (kind, serving API, tags).
    let models = lexi_host::models::model_summaries(&provider).map_err(|e| e.to_string())?;
    let models: Vec<Value> = models
        .into_iter()
        .map(|m| {
            json!({
                "id": m["id"],
                "name": m["name"],
                "description": m["description"],
                "default": m["default"],
                "aliases": m["aliases"],
                "max_tokens": m["metadata"].get("max_tokens"),
                "context_window": m["metadata"].get("context_window"),
                "kind": m["kind"],
                "api": m["api"],
                "tags": m["tags"],
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
/// (a live call to the provider's models endpoint). With `cached=1`, the list
/// from the last fetch instead (no network), plus its `fetched_at` (Unix secs).
async fn models_hosted(path: &str) -> Result<Value, String> {
    let provider = require_provider(path)?;
    let cached = query_param(path, "cached").is_some_and(|v| v == "1" || v == "true");
    // The catalog, each entry with kind / api / tags (a live fetch refreshes the cache).
    let models = lexi_host::models::hosted_summaries(&provider, cached, &[])
        .await
        .map_err(|e| e.to_string())?;
    let mut out = json!({ "provider": provider, "models": models });
    if cached {
        let catalog = lexi_host::hosted::cached_catalog(&provider).map_err(|e| e.to_string())?;
        out["fetched_at"] = catalog
            .and_then(|c| c.get("fetched_at").cloned())
            .unwrap_or(Value::Null);
    }
    Ok(out)
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

/// The shipped GUI themes; copied to `themes.json` in the Lexi home on first
/// use, where the user can edit them.
const SHIPPED_THEMES: &str = include_str!("../themes.json");

/// `GET /api/themes` — the GUI themes from `themes.json` in the Lexi home, the
/// default theme, and the selected one (`theme` in `config.json`).
fn themes() -> Value {
    let selected = config_get()
        .get("theme")
        .and_then(Value::as_str)
        .map(str::to_string);
    themes_in(&config::lexi_home(), selected.as_deref())
}

/// The themes in `<home>/themes.json`, writing the shipped file there first
/// when there is none. An unreadable or empty file falls back to the shipped
/// themes, with a `warning`. `selected` is used when it names a theme, else
/// the file's `default`, else its first theme.
fn themes_in(home: &std::path::Path, selected: Option<&str>) -> Value {
    let path = home.join("themes.json");
    let shipped: Value =
        serde_json::from_str(SHIPPED_THEMES).expect("the shipped themes.json is valid JSON");
    let mut warning: Option<String> = None;
    let file = if path.exists() {
        let parsed = std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|t| serde_json::from_str::<Value>(&t).map_err(|e| e.to_string()));
        match parsed {
            Ok(v)
                if v.get("themes")
                    .and_then(Value::as_array)
                    .is_some_and(|t| !t.is_empty()) =>
            {
                v
            }
            Ok(_) => {
                warning = Some(format!(
                    "{} lists no themes; using the built-in ones.",
                    path.display()
                ));
                shipped.clone()
            }
            Err(e) => {
                warning = Some(format!(
                    "Couldn't read {} ({e}); using the built-in themes.",
                    path.display()
                ));
                shipped.clone()
            }
        }
    } else {
        if let Err(e) =
            std::fs::create_dir_all(home).and_then(|_| std::fs::write(&path, SHIPPED_THEMES))
        {
            warning = Some(format!("Couldn't create {} ({e}).", path.display()));
        }
        shipped.clone()
    };
    let themes: Vec<Value> = file["themes"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|t| t.get("id").and_then(Value::as_str).is_some() && t["vars"].is_object())
        .collect();
    let has = |id: &str| themes.iter().any(|t| t["id"] == id);
    let first = themes.first().and_then(|t| t["id"].as_str()).unwrap_or("");
    let default = file
        .get("default")
        .and_then(Value::as_str)
        .filter(|d| has(d))
        .unwrap_or(first)
        .to_string();
    let selected = selected
        .filter(|s| has(s))
        .map(str::to_string)
        .unwrap_or_else(|| default.clone());
    json!({
        "path": path.display().to_string(),
        "default": default,
        "selected": selected,
        "themes": themes,
        "elements": file.get("elements").cloned().unwrap_or(Value::Null),
        "warning": warning,
    })
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
/// Tauri channel. Providers stream **concurrently** — their events are tagged by
/// provider, so interleaving is fine and the slowest provider no longer blocks
/// the others. A close sentinel is sent once all providers finish.
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

    let jobs = providers.iter().filter_map(Value::as_str).map(|provider| {
        let provider = provider.to_string();
        let model = models
            .get(&provider)
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let ch = on_event.clone();
        async move {
            let _ = ch.send(sse("provider_start", json!({ "provider": provider })));

            let cb_ch = ch.clone();
            let p = provider.clone();
            let result = lexi_host::respond_stream_text(
                &provider,
                prompt,
                "user",
                model.as_deref(),
                &[],
                move |frag| {
                    let _ = cb_ch.send(sse("delta", json!({ "provider": p, "text": frag })));
                },
            )
            .await;

            let done = match result {
                Ok(_) => json!({ "provider": provider }),
                Err(e) => json!({ "provider": provider, "error": e.to_string() }),
            };
            let _ = ch.send(sse("provider_done", done));
        }
    });

    futures_util::future::join_all(jobs).await;

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

    // Fan out over the selected providers concurrently; `join_all` preserves the
    // input order, so the rendered responses stay stable.
    let jobs = providers.iter().filter_map(Value::as_str).map(|provider| {
        let model = models
            .get(provider)
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty());
        async move {
            match lexi_host::respond_text(provider, prompt, "user", model, &[]).await {
                Ok(r) => json!({ "provider": provider, "output": r.text, "stderr": "" }),
                Err(e) => json!({ "provider": provider, "error": e.to_string() }),
            }
        }
    });
    let responses = futures_util::future::join_all(jobs).await;
    Ok(json!({ "responses": responses }))
}

// --- Images tab ---------------------------------------------------------------
// The same pipeline as `lexi respond --output image` / `lexi generate`: the
// provider's image binding picks the API and default model, the settings are
// that API's schema flags (`x-cli`), and images are saved by
// `lexi_host::generate::save_artifacts` (never overwriting a file).

/// Where generated images go: `images_dir` in `config.json` (`~` expanded),
/// else `~/Pictures/Lexi`.
fn images_dir() -> std::path::PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
    let configured = config::load_config()
        .ok()
        .and_then(|c| {
            c.get("images_dir")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .filter(|s| !s.trim().is_empty());
    match configured {
        Some(dir) => match dir.strip_prefix("~/") {
            Some(rest) => home.join(rest),
            None if dir == "~" => home,
            None => std::path::PathBuf::from(dir),
        },
        None => home.join("Pictures").join("Lexi"),
    }
}

/// `GET /api/images/providers` — every provider, whether it can make images
/// (an API that produces them), and its default image model; plus the folder
/// images are saved to.
fn images_providers() -> Result<Value, String> {
    let cfg = config::load_providers().map_err(|e| e.to_string())?;
    let names: Vec<String> = cfg
        .get("providers")
        .and_then(Value::as_object)
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default();
    let providers: Vec<Value> = names
        .iter()
        .map(
            |name| match lexi_host::resolve_model(name, None, Some("image")) {
                Ok(r) => {
                    json!({ "name": name, "images": true, "default_model": r.model, "api": r.api })
                }
                Err(e) => json!({ "name": name, "images": false, "reason": e.to_string() }),
            },
        )
        .collect();
    Ok(json!({
        "providers": providers,
        "images_dir": images_dir().display().to_string(),
    }))
}

/// `GET /api/images/models?provider=X` — the models that produce images
/// (`out:image`): configured ones, plus those in the cached catalog (usable
/// unconfigured). No network call.
async fn images_models(path: &str) -> Result<Value, String> {
    let provider = require_provider(path)?;
    let has_out_image = |m: &Value| {
        m.get("tags")
            .and_then(Value::as_array)
            .is_some_and(|t| t.iter().any(|t| t == "out:image"))
    };
    let mut models: Vec<Value> = Vec::new();
    for m in lexi_host::models::model_summaries(&provider).map_err(|e| e.to_string())? {
        if has_out_image(&m) {
            models.push(json!({ "id": m["id"], "name": m["name"], "configured": true }));
        }
    }
    // The catalog is optional: no cached listing just means no extra models.
    if let Ok(hosted) =
        lexi_host::models::hosted_summaries(&provider, true, &["out:image".to_string()]).await
    {
        for m in hosted {
            let Some(id) = m.get("id").and_then(Value::as_str) else {
                continue;
            };
            if models.iter().all(|c| c["id"] != id) {
                let name = m.get("display_name").or_else(|| m.get("name")).cloned();
                models.push(json!({ "id": id, "name": name, "configured": false }));
            }
        }
    }
    let default = lexi_host::resolve_model(&provider, None, Some("image"))
        .ok()
        .map(|r| r.model);
    Ok(json!({ "provider": provider, "default_model": default, "models": models }))
}

/// `GET /api/images/options?provider=X[&model=M]` — the settings the image API
/// serving that model accepts (its schema flags), and the model it resolves to.
fn images_options(path: &str) -> Result<Value, String> {
    let provider = require_provider(path)?;
    let model = query_param(path, "model").filter(|m| !m.is_empty());
    let r = lexi_host::resolve_model(&provider, model.as_deref(), Some("image"))
        .map_err(|e| e.to_string())?;
    // The prompt and model are set by the tab itself.
    let options: Vec<Value> = lexi_core::cli_flags(&r.effective)
        .into_iter()
        .filter(|f| !matches!(f["path"].as_str(), Some("prompt" | "model")))
        .collect();
    Ok(json!({
        "provider": provider,
        "model": r.model,
        "api": r.api,
        "options": options,
    }))
}

/// `POST /api/images/generate` — `{provider, model?, prompt, options: {path: value}}`.
/// Generates, saves each image to the images folder, and returns them with
/// their data inline (`data_url`) for the page to show.
async fn images_generate(body: Value) -> Result<Value, String> {
    use base64::prelude::*;
    let provider = require_field(&body, "provider")?;
    let prompt = require_field(&body, "prompt")?;
    let model = body
        .get("model")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let set_paths: Vec<(String, Value)> = body
        .get("options")
        .and_then(Value::as_object)
        .map(|o| {
            o.iter()
                .filter(|(_, v)| !v.is_null() && *v != "")
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect()
        })
        .unwrap_or_default();
    let req = lexi_host::ChatRequest {
        prompt: json!({ "prompt": prompt, "role": "user" }),
        model,
        output: Some("image".into()),
        set_paths,
        ..Default::default()
    };
    let chat = lexi_host::prepare_chat(&provider, &req, lexi_host::ChatMode::Buffered)
        .map_err(|e| e.to_string())?;
    let res = lexi_host::run_chat(&chat, |_| {})
        .await
        .map_err(|e| e.to_string())?;
    let dir = images_dir();
    let saved = lexi_host::generate::save_artifacts(&chat, &res, &dir)
        .await
        .map_err(|e| e.to_string())?;
    if saved.is_empty() {
        let text = res.text.trim();
        return Err(if text.is_empty() {
            "the provider returned no images".to_string()
        } else {
            format!("the provider returned no images: {text}")
        });
    }
    let images: Vec<Value> = saved
        .iter()
        .map(|a| {
            let mut v = a.to_value();
            if let Ok(bytes) = std::fs::read(&a.path) {
                v["data_url"] = Value::from(format!(
                    "data:{};base64,{}",
                    a.media_type,
                    BASE64_STANDARD.encode(bytes)
                ));
            }
            v
        })
        .collect();
    Ok(json!({
        "provider": provider,
        "model": chat.model,
        "images": images,
        "warnings": chat.warnings,
    }))
}

/// `POST /api/images/reveal` — `{path}` shows that file in the file manager;
/// `{}` opens the images folder (creating it first).
fn images_reveal(body: Value) -> Result<Value, String> {
    use std::process::Command;
    let file = body
        .get("path")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty());
    let status = match file {
        Some(path) => {
            if !std::path::Path::new(path).exists() {
                return Err(format!("{path} no longer exists"));
            }
            if cfg!(target_os = "macos") {
                Command::new("open").args(["-R", path]).status()
            } else if cfg!(target_os = "windows") {
                Command::new("explorer")
                    .arg(format!("/select,{path}"))
                    .status()
            } else {
                let parent = std::path::Path::new(path)
                    .parent()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| ".".into());
                Command::new("xdg-open").arg(parent).status()
            }
        }
        None => {
            let dir = images_dir();
            std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            let opener = if cfg!(target_os = "macos") {
                "open"
            } else if cfg!(target_os = "windows") {
                "explorer"
            } else {
                "xdg-open"
            };
            Command::new(opener).arg(&dir).status()
        }
    };
    status.map_err(|e| e.to_string())?;
    Ok(json!({}))
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
    fn themes_are_seeded_and_the_selection_falls_back_to_the_default() {
        use super::themes_in;
        let home = tempfile::tempdir().unwrap();
        // First use writes the shipped file; an unknown theme falls back to the default.
        let t = themes_in(home.path(), Some("dark-green"));
        assert!(home.path().join("themes.json").exists());
        assert_eq!(t["default"], "dark-cyan");
        assert_eq!(t["selected"], "dark-cyan");
        assert!(t["warning"].is_null());
        let ids: Vec<&str> = t["themes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["id"].as_str().unwrap())
            .collect();
        assert_eq!(
            ids,
            ["dark-cyan", "dark-purple", "nvidia-dark", "nvidia-light"]
        );
        // A saved choice that exists is kept.
        assert_eq!(
            themes_in(home.path(), Some("nvidia-dark"))["selected"],
            "nvidia-dark"
        );
        // The user's file is what's read: an added theme shows up.
        std::fs::write(
            home.path().join("themes.json"),
            r##"{"default":"mine","themes":[{"id":"mine","name":"Mine","vars":{"--bg":"#000"}}]}"##,
        )
        .unwrap();
        let t = themes_in(home.path(), None);
        assert_eq!(t["selected"], "mine");
        assert_eq!(t["themes"].as_array().unwrap().len(), 1);
        // A broken file falls back to the shipped themes, with a warning.
        std::fs::write(home.path().join("themes.json"), "{not json").unwrap();
        let t = themes_in(home.path(), Some("nvidia-light"));
        assert_eq!(t["selected"], "nvidia-light");
        assert!(t["warning"]
            .as_str()
            .unwrap()
            .contains("using the built-in themes"));
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

    /// The Images tab's calls end to end against a mock image API: the
    /// provider is found, its settings listed, and a generation sends the
    /// chosen settings and saves the images to `images_dir`.
    #[tokio::test]
    async fn images_generate_against_a_mock_provider() {
        use serde_json::json;
        use wiremock::matchers::{body_partial_json, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/images/generations"))
            .and(body_partial_json(
                json!({ "model": "img-model", "size": "1024x1024", "n": 2 }),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "created": 1,
                "data": [
                    { "b64_json": "iVBORw0KGgo=", "revised_prompt": "a calm fox" },
                    { "b64_json": "iVBORw0KGgo=" }
                ]
            })))
            .mount(&server)
            .await;

        let home = tempfile::tempdir().unwrap();
        let out = home.path().join("pictures");
        std::env::set_var("LEXI_HOME", home.path());
        std::fs::create_dir_all(home.path().join("providers")).unwrap();
        std::fs::write(
            home.path().join("providers/providers-config.yaml"),
            format!(
                "providers:\n  mockimg:\n    api_key: k\n    apis:\n      - api: image-generations\n        url: {}\n        default_model: img-model\n",
                server.uri()
            ),
        )
        .unwrap();
        std::fs::write(
            home.path().join("config.json"),
            serde_json::to_string(&json!({ "images_dir": out.display().to_string() })).unwrap(),
        )
        .unwrap();

        let providers = super::handle("GET", "/api/images/providers", None)
            .await
            .unwrap();
        let mock = providers["providers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == "mockimg")
            .unwrap();
        assert_eq!(mock["images"], true);
        assert_eq!(mock["default_model"], "img-model");
        assert_eq!(providers["images_dir"], out.display().to_string());

        // A provider with only an image API is left out of the Chat tab.
        let all = super::handle("GET", "/api/providers", None).await.unwrap();
        let mock = all["providers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == "mockimg")
            .unwrap();
        assert_eq!(mock["chat"], false);

        let opts = super::handle("GET", "/api/images/options?provider=mockimg", None)
            .await
            .unwrap();
        let paths: Vec<&str> = opts["options"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|o| o["path"].as_str())
            .collect();
        assert!(paths.contains(&"size") && paths.contains(&"n"), "{paths:?}");
        assert!(!paths.contains(&"prompt") && !paths.contains(&"model"));

        let res = super::handle(
            "POST",
            "/api/images/generate",
            Some(json!({
                "provider": "mockimg",
                "prompt": "A calm fox",
                "options": { "size": "1024x1024", "n": 2, "quality": "" }
            })),
        )
        .await
        .unwrap();
        let images = res["images"].as_array().unwrap();
        assert_eq!(images.len(), 2);
        assert_eq!(images[0]["revised_prompt"], "a calm fox");
        assert!(images[0]["data_url"]
            .as_str()
            .unwrap()
            .starts_with("data:image/png;base64,"));
        assert!(out.join("a-calm-fox-1.png").exists() && out.join("a-calm-fox-2.png").exists());
    }
}
