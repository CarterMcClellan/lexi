//! Config, API-definition, and API-key loading.
//!
//! Reads `~/.lexi-cli/` (overridable via `LEXI_HOME`), shared with the Python
//! CLI. The API definitions are embedded in the binary, with a per-user
//! override under `~/.lexi-cli/apis/`.

use std::path::{Path, PathBuf};

use serde_json::{Map as JsonMap, Value};

use crate::HostError;

/// A JSON object map (shorthand used by the config writers).
type Map = JsonMap<String, Value>;

/// API definitions embedded at build time (the same files lexi-core bundles).
const EMBEDDED_APIS: &[(&str, &str)] = &[
    (
        "messages",
        include_str!("../../lexi-core/apis/messages.yaml"),
    ),
    (
        "chat-completions",
        include_str!("../../lexi-core/apis/chat-completions.yaml"),
    ),
    (
        "responses",
        include_str!("../../lexi-core/apis/responses.yaml"),
    ),
    (
        "image-generations",
        include_str!("../../lexi-core/apis/image-generations.yaml"),
    ),
];

/// The Lexi config dir: `$LEXI_HOME`, else `~/.lexi-cli`.
pub fn lexi_home() -> PathBuf {
    if let Ok(h) = std::env::var("LEXI_HOME") {
        return PathBuf::from(h);
    }
    dirs::home_dir().unwrap_or_default().join(".lexi-cli")
}

/// The names of the API definitions shipped (embedded) in the binary.
pub fn embedded_api_names() -> Vec<&'static str> {
    EMBEDDED_APIS.iter().map(|(n, _)| *n).collect()
}

/// The raw YAML text of a shipped API definition, if `name` is one.
fn embedded_api_text(name: &str) -> Option<&'static str> {
    EMBEDDED_APIS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, t)| *t)
}

/// Load an API definition by name.
///
/// Shipped definitions always use the current **embedded** copy (so a stale
/// `~/.lexi-cli/apis/` seeded by an older CLI can't mask new fields); the user
/// dir supplies only *additional* custom definitions.
pub fn load_api_def(name: &str) -> Result<Value, HostError> {
    if let Some((_, text)) = EMBEDDED_APIS.iter().find(|(n, _)| *n == name) {
        return Ok(serde_yaml::from_str(text)?);
    }
    let user = lexi_home().join("apis").join(format!("{name}.yaml"));
    if user.exists() {
        return Ok(serde_yaml::from_str(&std::fs::read_to_string(&user)?)?);
    }
    Err(HostError::UnknownApi(name.to_string()))
}

/// Load `providers-config.yaml`.
pub fn load_providers() -> Result<Value, HostError> {
    let path = lexi_home().join("providers").join("providers-config.yaml");
    let text = std::fs::read_to_string(&path)
        .map_err(|_| HostError::Config(format!("no providers-config at {}", path.display())))?;
    Ok(serde_yaml::from_str(&text)?)
}

/// Deep-merge `overlay` over `base`: dicts recurse, lists/scalars are replaced
/// (the provider-override semantics from the spec).
pub fn deep_merge(base: &Value, overlay: &Value) -> Value {
    match (base, overlay) {
        (Value::Object(b), Value::Object(o)) => {
            let mut out = b.clone();
            for (k, v) in o {
                let merged = match out.get(k) {
                    Some(existing) => deep_merge(existing, v),
                    None => v.clone(),
                };
                out.insert(k.clone(), merged);
            }
            Value::Object(out)
        }
        _ => overlay.clone(),
    }
}

/// Effective config = the referenced API definition with the provider entry
/// merged over it (provider wins).
pub fn effective_config(provider_name: &str, provider_cfg: &Value) -> Result<Value, HostError> {
    let api_name = provider_cfg
        .get("api")
        .and_then(Value::as_str)
        .ok_or_else(|| HostError::MissingApi(provider_name.to_string()))?;
    let api_def = load_api_def(api_name)?;
    Ok(deep_merge(&api_def, provider_cfg))
}

/// Resolve a provider's API key: an explicit `api_key` (with `${VAR}` env
/// expansion), else `~/.lexi-cli/providers/<name>/api-key`.
pub fn resolve_api_key(provider_name: &str, provider_cfg: &Value) -> Result<String, HostError> {
    if let Some(raw) = provider_cfg.get("api_key").and_then(Value::as_str) {
        if let Some(var) = raw.strip_prefix("${").and_then(|s| s.strip_suffix('}')) {
            if let Ok(val) = std::env::var(var) {
                if !val.is_empty() {
                    return Ok(val);
                }
            }
        } else if !raw.is_empty() {
            return Ok(raw.to_string());
        }
    }
    let key_file = lexi_home()
        .join("providers")
        .join(provider_name)
        .join("api-key");
    if key_file.exists() {
        let s = std::fs::read_to_string(&key_file)?.trim().to_string();
        if !s.is_empty() {
            return Ok(s);
        }
    }
    Err(HostError::MissingApiKey(provider_name.to_string()))
}

/// Extract provider names from a loaded providers config (declaration order).
pub fn provider_names(providers: &Value) -> Vec<String> {
    providers
        .get("providers")
        .and_then(Value::as_object)
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default()
}

/// Load the providers config and return the configured provider names.
pub fn list_providers() -> Result<Vec<String>, HostError> {
    Ok(provider_names(&load_providers()?))
}

/// Read a per-provider YAML file (e.g. `configured-models.yaml`) under `home`,
/// returning the parsed value or `Value::Null` if the file is absent.
fn read_provider_file(home: &Path, provider: &str, file: &str) -> Result<Value, HostError> {
    let path = home.join("providers").join(provider).join(file);
    if !path.exists() {
        return Ok(Value::Null);
    }
    Ok(serde_yaml::from_str(&std::fs::read_to_string(&path)?)?)
}

/// A configured model for a provider, as surfaced to the GUI/CLI.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ModelInfo {
    pub id: String,
    pub description: Option<String>,
    pub default: bool,
    /// Aliases that resolve to this model id.
    pub aliases: Vec<String>,
}

/// Load a provider's configured models, enriched with its default model and
/// aliases. Reads `configured-models.yaml`, `default-model.yaml`, and
/// `model-aliases.yaml` under `providers/<provider>/`. Preserves declaration
/// order; returns an empty list if the provider has no configured models.
pub fn load_models(provider: &str) -> Result<Vec<ModelInfo>, HostError> {
    load_models_in(&lexi_home(), provider)
}

fn load_models_in(home: &Path, provider: &str) -> Result<Vec<ModelInfo>, HostError> {
    let models = read_provider_file(home, provider, "configured-models.yaml")?;
    let default = read_provider_file(home, provider, "default-model.yaml")?
        .get("default_model")
        .and_then(Value::as_str)
        .map(str::to_string);
    let aliases = read_provider_file(home, provider, "model-aliases.yaml")?;
    let aliases = aliases.get("aliases").and_then(Value::as_object);

    let entries = models.get("models").and_then(Value::as_object);
    let Some(entries) = entries else {
        return Ok(Vec::new());
    };

    Ok(entries
        .iter()
        .map(|(id, meta)| {
            let alias_list = aliases
                .map(|a| {
                    a.iter()
                        .filter(|(_, target)| target.as_str() == Some(id.as_str()))
                        .map(|(alias, _)| alias.clone())
                        .collect()
                })
                .unwrap_or_default();
            ModelInfo {
                id: id.clone(),
                description: meta
                    .get("description")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                default: default.as_deref() == Some(id.as_str()),
                aliases: alias_list,
            }
        })
        .collect())
}

/// Path to a per-provider config file under `providers/<provider>/`.
fn provider_file(home: &Path, provider: &str, file: &str) -> PathBuf {
    home.join("providers").join(provider).join(file)
}

/// Write `value` as YAML, creating the parent dir if needed.
fn write_yaml(path: &Path, value: &Value) -> Result<(), HostError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_yaml::to_string(value)?)?;
    Ok(())
}

/// The provider's current default model id, if set.
pub fn default_model(provider: &str) -> Result<Option<String>, HostError> {
    default_model_in(&lexi_home(), provider)
}

fn default_model_in(home: &Path, provider: &str) -> Result<Option<String>, HostError> {
    Ok(read_provider_file(home, provider, "default-model.yaml")?
        .get("default_model")
        .and_then(Value::as_str)
        .map(str::to_string))
}

/// Set (or clear, with `None`) the provider's default model.
pub fn set_default_model(provider: &str, model: Option<&str>) -> Result<(), HostError> {
    set_default_model_in(&lexi_home(), provider, model)
}

fn set_default_model_in(home: &Path, provider: &str, model: Option<&str>) -> Result<(), HostError> {
    let data = match model {
        Some(m) => serde_json::json!({ "default_model": m }),
        None => Value::Object(Map::new()),
    };
    write_yaml(&provider_file(home, provider, "default-model.yaml"), &data)
}

/// The provider's `alias -> model` map (empty if none configured).
pub fn model_aliases(provider: &str) -> Result<Map, HostError> {
    model_aliases_in(&lexi_home(), provider)
}

fn model_aliases_in(home: &Path, provider: &str) -> Result<Map, HostError> {
    let raw = read_provider_file(home, provider, "model-aliases.yaml")?;
    // Files use either a top-level `aliases:` wrapper or a flat map.
    let map = raw
        .get("aliases")
        .and_then(Value::as_object)
        .or_else(|| raw.as_object())
        .cloned()
        .unwrap_or_default();
    Ok(map)
}

/// Save the alias map, preserving the file's existing wrapper format (a
/// top-level `aliases:` key if present, else a flat map — matching the CLI).
fn save_model_aliases_in(home: &Path, provider: &str, aliases: &Map) -> Result<(), HostError> {
    let path = provider_file(home, provider, "model-aliases.yaml");
    let wrapped = path.exists()
        && read_provider_file(home, provider, "model-aliases.yaml")?
            .get("aliases")
            .is_some();
    let data = if wrapped {
        serde_json::json!({ "aliases": aliases })
    } else {
        Value::Object(aliases.clone())
    };
    write_yaml(&path, &data)
}

/// Set an alias to point at a model.
pub fn set_model_alias(provider: &str, alias: &str, model: &str) -> Result<(), HostError> {
    set_model_alias_in(&lexi_home(), provider, alias, model)
}

fn set_model_alias_in(
    home: &Path,
    provider: &str,
    alias: &str,
    model: &str,
) -> Result<(), HostError> {
    let mut aliases = model_aliases_in(home, provider)?;
    aliases.insert(alias.to_string(), Value::String(model.to_string()));
    save_model_aliases_in(home, provider, &aliases)
}

/// Remove an alias. Errors if it doesn't exist.
pub fn delete_model_alias(provider: &str, alias: &str) -> Result<(), HostError> {
    delete_model_alias_in(&lexi_home(), provider, alias)
}

fn delete_model_alias_in(home: &Path, provider: &str, alias: &str) -> Result<(), HostError> {
    let mut aliases = model_aliases_in(home, provider)?;
    if aliases.remove(alias).is_none() {
        return Err(HostError::Config(format!(
            "alias {alias:?} not found for {provider}"
        )));
    }
    save_model_aliases_in(home, provider, &aliases)
}

/// Add or update a model's metadata (merging over any existing entry), optionally
/// making it the default. Returns whether the model already existed. Mirrors the
/// CLI's `models add`/`set` (both upsert-merge).
pub fn upsert_model(
    provider: &str,
    model: &str,
    meta: &Map,
    set_default: bool,
) -> Result<bool, HostError> {
    upsert_model_in(&lexi_home(), provider, model, meta, set_default)
}

fn upsert_model_in(
    home: &Path,
    provider: &str,
    model: &str,
    meta: &Map,
    set_default: bool,
) -> Result<bool, HostError> {
    let mut data = match read_provider_file(home, provider, "configured-models.yaml")? {
        Value::Object(m) => m,
        _ => Map::new(),
    };
    let models = data
        .entry("models")
        .or_insert_with(|| Value::Object(Map::new()));
    if !models.is_object() {
        *models = Value::Object(Map::new());
    }
    let models = models.as_object_mut().unwrap();
    let existed = models.contains_key(model);
    let mut merged = models
        .get(model)
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    for (k, v) in meta {
        merged.insert(k.clone(), v.clone());
    }
    models.insert(model.to_string(), Value::Object(merged));

    write_yaml(
        &provider_file(home, provider, "configured-models.yaml"),
        &Value::Object(data),
    )?;
    if set_default {
        set_default_model_in(home, provider, Some(model))?;
    }
    Ok(existed)
}

/// Remove a model. Errors if absent. If it was the default, the default falls to
/// the first remaining model (or is cleared when none remain).
pub fn delete_model(provider: &str, model: &str) -> Result<(), HostError> {
    delete_model_in(&lexi_home(), provider, model)
}

fn delete_model_in(home: &Path, provider: &str, model: &str) -> Result<(), HostError> {
    let mut data = match read_provider_file(home, provider, "configured-models.yaml")? {
        Value::Object(m) => m,
        _ => Map::new(),
    };
    let new_default;
    {
        let models = data
            .get_mut("models")
            .and_then(Value::as_object_mut)
            .ok_or_else(|| {
                HostError::Config(format!("model {model:?} not found for {provider}"))
            })?;
        if models.remove(model).is_none() {
            return Err(HostError::Config(format!(
                "model {model:?} not found for {provider}"
            )));
        }
        new_default = models.keys().next().cloned();
    }
    let was_default = default_model_in(home, provider)?.as_deref() == Some(model);
    write_yaml(
        &provider_file(home, provider, "configured-models.yaml"),
        &Value::Object(data),
    )?;
    if was_default {
        set_default_model_in(home, provider, new_default.as_deref())?;
    }
    Ok(())
}

/// Load `config.json` (the CLI's app settings), or an empty object if absent.
/// Caller-supplied defaults are layered on top by the frontend.
pub fn load_config() -> Result<Value, HostError> {
    load_config_in(&lexi_home())
}

fn load_config_in(home: &Path) -> Result<Value, HostError> {
    let path = home.join("config.json");
    if !path.exists() {
        return Ok(Value::Object(Default::default()));
    }
    Ok(serde_json::from_str(&std::fs::read_to_string(&path)?)?)
}

/// Load the saved prompts as `(name, body)` pairs, where `body` is the prompt's
/// JSON object (`prompt`, `max_tokens`, …). Reads `prompts/*.json`; the active
/// prompt is the one stored under the `$$` name. Sorted by name for stable order.
pub fn load_prompts() -> Result<Vec<(String, Value)>, HostError> {
    load_prompts_in(&lexi_home())
}

fn load_prompts_in(home: &Path) -> Result<Vec<(String, Value)>, HostError> {
    let dir = home.join("prompts");
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut entries = Vec::new();
    for ent in std::fs::read_dir(&dir)? {
        let path = ent?.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let Some(name) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let body: Value = serde_json::from_str(&std::fs::read_to_string(&path)?)?;
        entries.push((name.to_string(), body));
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(entries)
}

/// Merge `updates` into `config.json` (created if absent) and return the result.
/// Existing keys not in `updates` are preserved.
pub fn set_config(updates: &Map) -> Result<Value, HostError> {
    set_config_in(&lexi_home(), updates)
}

fn set_config_in(home: &Path, updates: &Map) -> Result<Value, HostError> {
    let mut cfg = match load_config_in(home)? {
        Value::Object(m) => m,
        _ => Map::new(),
    };
    for (k, v) in updates {
        cfg.insert(k.clone(), v.clone());
    }
    write_json(&home.join("config.json"), &Value::Object(cfg.clone()))?;
    Ok(Value::Object(cfg))
}

/// The `prompts/` dir name of the active prompt slot.
const ACTIVE_PROMPT: &str = "$$";

/// Save a prompt under `name` (or the active `$$` slot). `body` is written
/// verbatim as the prompt's JSON object (e.g. `{prompt, max_tokens, …}`).
pub fn save_prompt(name: &str, body: &Value) -> Result<(), HostError> {
    save_prompt_in(&lexi_home(), name, body)
}

fn save_prompt_in(home: &Path, name: &str, body: &Value) -> Result<(), HostError> {
    write_json(&prompt_path(home, name)?, body)
}

/// Delete the named prompt. Errors if the name is invalid or the file is absent.
pub fn delete_prompt(name: &str) -> Result<(), HostError> {
    delete_prompt_in(&lexi_home(), name)
}

fn delete_prompt_in(home: &Path, name: &str) -> Result<(), HostError> {
    let path = prompt_path(home, name)?;
    if !path.exists() {
        return Err(HostError::Config(format!("prompt {name:?} not found")));
    }
    std::fs::remove_file(&path)?;
    Ok(())
}

/// Make `name` the active prompt by copying its body into the `$$` slot.
pub fn set_active_prompt(name: &str) -> Result<(), HostError> {
    set_active_prompt_in(&lexi_home(), name)
}

fn set_active_prompt_in(home: &Path, name: &str) -> Result<(), HostError> {
    let src = prompt_path(home, name)?;
    if !src.exists() {
        return Err(HostError::Config(format!("prompt {name:?} not found")));
    }
    let body: Value = serde_json::from_str(&std::fs::read_to_string(&src)?)?;
    save_prompt_in(home, ACTIVE_PROMPT, &body)
}

/// Resolve a prompt name to its `prompts/<name>.json` path, rejecting names that
/// contain path separators or `..` (so a name can't escape the prompts dir).
fn prompt_path(home: &Path, name: &str) -> Result<PathBuf, HostError> {
    if name.is_empty()
        || name.contains('/')
        || name.contains('\\')
        || name.split(['/', '\\']).any(|seg| seg == "..")
    {
        return Err(HostError::Config(format!("invalid prompt name {name:?}")));
    }
    Ok(home.join("prompts").join(format!("{name}.json")))
}

/// Write `value` as pretty JSON, creating the parent dir if needed.
fn write_json(path: &Path, value: &Value) -> Result<(), HostError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_string_pretty(value)?)?;
    Ok(())
}

/// List available API definitions as `(name, description)` pairs — the union of
/// shipped (embedded) definitions and any custom ones under `~/.lexi-cli/apis/`.
pub fn list_api_defs() -> Result<Vec<(String, Option<String>)>, HostError> {
    list_api_defs_in(&lexi_home())
}

fn list_api_defs_in(home: &Path) -> Result<Vec<(String, Option<String>)>, HostError> {
    use std::collections::BTreeSet;
    let mut names: BTreeSet<String> = embedded_api_names().iter().map(|s| s.to_string()).collect();
    let dir = home.join("apis");
    if dir.exists() {
        for ent in std::fs::read_dir(&dir)? {
            let path = ent?.path();
            if path.extension().and_then(|s| s.to_str()) == Some("yaml") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    names.insert(stem.to_string());
                }
            }
        }
    }
    Ok(names
        .into_iter()
        .map(|name| {
            let desc = load_api_raw_in(home, &name)
                .ok()
                .and_then(|t| serde_yaml::from_str::<Value>(&t).ok())
                .and_then(|v| {
                    v.get("description")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                });
            (name, desc)
        })
        .collect())
}

/// Return the raw YAML text of an API definition. Shipped definitions resolve to
/// their embedded copy (matching [`load_api_def`]); otherwise the user file.
pub fn load_api_raw(name: &str) -> Result<String, HostError> {
    load_api_raw_in(&lexi_home(), name)
}

fn load_api_raw_in(home: &Path, name: &str) -> Result<String, HostError> {
    if let Some(text) = embedded_api_text(name) {
        return Ok(text.to_string());
    }
    let user = home.join("apis").join(format!("{name}.yaml"));
    if user.exists() {
        return Ok(std::fs::read_to_string(&user)?);
    }
    Err(HostError::UnknownApi(name.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn provider_names_in_order() {
        let cfg = json!({ "providers": { "anthropic": {}, "openai": {}, "nvidia": {} } });
        assert_eq!(provider_names(&cfg), ["anthropic", "openai", "nvidia"]);
    }

    #[test]
    fn load_models_enriches_with_default_and_aliases() {
        let dir = tempfile::tempdir().unwrap();
        let pdir = dir.path().join("providers").join("openai");
        std::fs::create_dir_all(&pdir).unwrap();
        std::fs::write(
            pdir.join("configured-models.yaml"),
            "models:\n  gpt-4o:\n    description: cap\n  gpt-4o-mini:\n    description: mini\n",
        )
        .unwrap();
        std::fs::write(pdir.join("default-model.yaml"), "default_model: gpt-4o\n").unwrap();
        std::fs::write(
            pdir.join("model-aliases.yaml"),
            "aliases:\n  gpt4: gpt-4o\n  mini: gpt-4o-mini\n",
        )
        .unwrap();

        let models = load_models_in(dir.path(), "openai").unwrap();
        assert_eq!(models.len(), 2);
        // Declaration order is preserved.
        assert_eq!(models[0].id, "gpt-4o");
        assert_eq!(models[0].description.as_deref(), Some("cap"));
        assert!(models[0].default);
        assert_eq!(models[0].aliases, ["gpt4"]);
        assert_eq!(models[1].id, "gpt-4o-mini");
        assert!(!models[1].default);
        assert_eq!(models[1].aliases, ["mini"]);
    }

    #[test]
    fn load_models_empty_for_unknown_provider() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load_models_in(dir.path(), "nope").unwrap().is_empty());
    }

    #[test]
    fn upsert_model_adds_then_merges_and_sets_default() {
        let dir = tempfile::tempdir().unwrap();
        let mut meta = Map::new();
        meta.insert("description".into(), json!("cap"));
        meta.insert("max_tokens".into(), json!(4096));

        // First upsert creates the entry and sets it default.
        let existed = upsert_model_in(dir.path(), "openai", "gpt-4o", &meta, true).unwrap();
        assert!(!existed);
        assert_eq!(
            default_model_in(dir.path(), "openai").unwrap().as_deref(),
            Some("gpt-4o")
        );

        // Second upsert merges new fields over the existing entry.
        let mut meta2 = Map::new();
        meta2.insert("context_window".into(), json!(128000));
        let existed = upsert_model_in(dir.path(), "openai", "gpt-4o", &meta2, false).unwrap();
        assert!(existed);
        let models = load_models_in(dir.path(), "openai").unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].description.as_deref(), Some("cap"));
        assert!(models[0].default);
    }

    #[test]
    fn delete_model_fixes_default_and_errors_when_absent() {
        let dir = tempfile::tempdir().unwrap();
        let empty = Map::new();
        upsert_model_in(dir.path(), "openai", "a", &empty, true).unwrap();
        upsert_model_in(dir.path(), "openai", "b", &empty, false).unwrap();

        // Deleting the default promotes the first remaining model.
        delete_model_in(dir.path(), "openai", "a").unwrap();
        assert_eq!(
            default_model_in(dir.path(), "openai").unwrap().as_deref(),
            Some("b")
        );

        // Deleting the last model clears the default.
        delete_model_in(dir.path(), "openai", "b").unwrap();
        assert_eq!(default_model_in(dir.path(), "openai").unwrap(), None);

        assert!(delete_model_in(dir.path(), "openai", "gone").is_err());
    }

    #[test]
    fn alias_set_delete_preserves_wrapper() {
        let dir = tempfile::tempdir().unwrap();
        let pdir = dir.path().join("providers").join("openai");
        std::fs::create_dir_all(&pdir).unwrap();
        std::fs::write(
            pdir.join("model-aliases.yaml"),
            "aliases:\n  gpt4: gpt-4o\n",
        )
        .unwrap();

        set_model_alias_in(dir.path(), "openai", "mini", "gpt-4o-mini").unwrap();
        let aliases = model_aliases_in(dir.path(), "openai").unwrap();
        assert_eq!(aliases["gpt4"], "gpt-4o");
        assert_eq!(aliases["mini"], "gpt-4o-mini");
        // Wrapper preserved on disk.
        let raw = std::fs::read_to_string(pdir.join("model-aliases.yaml")).unwrap();
        assert!(raw.contains("aliases:"));

        delete_model_alias_in(dir.path(), "openai", "gpt4").unwrap();
        assert!(!model_aliases_in(dir.path(), "openai")
            .unwrap()
            .contains_key("gpt4"));
        assert!(delete_model_alias_in(dir.path(), "openai", "nope").is_err());
    }

    #[test]
    fn load_config_reads_json_or_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load_config_in(dir.path()).unwrap(), json!({}));
        std::fs::write(dir.path().join("config.json"), r#"{"edit_mode":"vi"}"#).unwrap();
        assert_eq!(
            load_config_in(dir.path()).unwrap(),
            json!({ "edit_mode": "vi" })
        );
    }

    #[test]
    fn set_config_merges_and_persists() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("config.json"), r#"{"theme":"dark"}"#).unwrap();

        let mut updates = Map::new();
        updates.insert("edit_mode".into(), json!("vi"));
        let merged = set_config_in(dir.path(), &updates).unwrap();
        assert_eq!(merged, json!({ "theme": "dark", "edit_mode": "vi" }));
        // Persisted to disk, with the prior key preserved.
        assert_eq!(load_config_in(dir.path()).unwrap(), merged);
    }

    #[test]
    fn set_config_creates_file_when_absent() {
        let dir = tempfile::tempdir().unwrap();
        let mut updates = Map::new();
        updates.insert("prompt".into(), json!("hi"));
        let merged = set_config_in(dir.path(), &updates).unwrap();
        assert_eq!(merged, json!({ "prompt": "hi" }));
    }

    #[test]
    fn save_and_delete_and_activate_prompt() {
        let dir = tempfile::tempdir().unwrap();
        let body = json!({ "prompt": "tell a joke", "max_tokens": 256, "role": "user" });
        save_prompt_in(dir.path(), "joke", &body).unwrap();

        let prompts = load_prompts_in(dir.path()).unwrap();
        assert_eq!(prompts, vec![("joke".to_string(), body.clone())]);

        // Activating copies the body into the $$ slot.
        set_active_prompt_in(dir.path(), "joke").unwrap();
        let active = load_prompts_in(dir.path()).unwrap();
        assert!(active.iter().any(|(n, b)| n == "$$" && *b == body));

        // Delete removes just the named file.
        delete_prompt_in(dir.path(), "joke").unwrap();
        let after = load_prompts_in(dir.path()).unwrap();
        assert_eq!(
            after.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(),
            ["$$"]
        );
    }

    #[test]
    fn prompt_ops_reject_bad_names_and_missing_files() {
        let dir = tempfile::tempdir().unwrap();
        assert!(save_prompt_in(dir.path(), "../escape", &json!({})).is_err());
        assert!(save_prompt_in(dir.path(), "a/b", &json!({})).is_err());
        assert!(save_prompt_in(dir.path(), "", &json!({})).is_err());
        assert!(delete_prompt_in(dir.path(), "nope").is_err());
        assert!(set_active_prompt_in(dir.path(), "nope").is_err());
    }

    #[test]
    fn load_prompts_reads_and_sorts() {
        let dir = tempfile::tempdir().unwrap();
        let pdir = dir.path().join("prompts");
        std::fs::create_dir_all(&pdir).unwrap();
        std::fs::write(pdir.join("$$.json"), r#"{"prompt":"active","role":"user"}"#).unwrap();
        std::fs::write(pdir.join("joke.json"), r#"{"prompt":"a joke"}"#).unwrap();
        std::fs::write(pdir.join("notes.txt"), "ignored").unwrap();

        let prompts = load_prompts_in(dir.path()).unwrap();
        assert_eq!(prompts.len(), 2);
        assert_eq!(prompts[0].0, "$$");
        assert_eq!(prompts[0].1["prompt"], "active");
        assert_eq!(prompts[1].0, "joke");
    }

    #[test]
    fn list_api_defs_unions_embedded_and_user() {
        let dir = tempfile::tempdir().unwrap();
        let adir = dir.path().join("apis");
        std::fs::create_dir_all(&adir).unwrap();
        std::fs::write(adir.join("custom.yaml"), "description: my api\n").unwrap();

        let defs = list_api_defs_in(dir.path()).unwrap();
        let names: Vec<&str> = defs.iter().map(|(n, _)| n.as_str()).collect();
        assert!(names.contains(&"messages")); // embedded
        assert!(names.contains(&"custom")); // user
        let custom = defs.iter().find(|(n, _)| n == "custom").unwrap();
        assert_eq!(custom.1.as_deref(), Some("my api"));
    }

    #[test]
    fn load_api_raw_prefers_embedded_then_user() {
        let dir = tempfile::tempdir().unwrap();
        // Shipped definition resolves to embedded text even without a user file.
        assert!(load_api_raw_in(dir.path(), "messages")
            .unwrap()
            .contains("description"));
        // Custom definition reads the user file.
        let adir = dir.path().join("apis");
        std::fs::create_dir_all(&adir).unwrap();
        std::fs::write(adir.join("custom.yaml"), "description: my api\n").unwrap();
        assert_eq!(
            load_api_raw_in(dir.path(), "custom").unwrap(),
            "description: my api\n"
        );
        assert!(load_api_raw_in(dir.path(), "nope").is_err());
    }

    #[test]
    fn provider_names_empty_when_absent() {
        assert!(provider_names(&json!({})).is_empty());
    }
}
