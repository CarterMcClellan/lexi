//! Config, API-definition, and API-key loading.
//!
//! Reads `~/.lexi-cli/` (overridable via `LEXI_HOME`), shared with the Python
//! CLI. The API definitions are embedded in the binary, with a per-user
//! override under `~/.lexi-cli/apis/`.

use std::path::PathBuf;

use serde_json::Value;

use crate::HostError;

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
