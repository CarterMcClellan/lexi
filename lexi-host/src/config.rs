//! Config, API-definition, and API-key loading.
//!
//! Reads `~/.lexi-cli/` (overridable via `LEXI_HOME`), shared with the Python
//! CLI. The API definitions are embedded in the binary, with a per-user
//! override under `~/.lexi-cli/apis/`.

use std::path::{Path, PathBuf};

use serde_json::{Map as JsonMap, Value};

use crate::keystore::{self, Lookup, Source};
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
    (
        "embeddings",
        include_str!("../../lexi-core/apis/embeddings.yaml"),
    ),
    (
        "nvidia-genai-image",
        include_str!("../../lexi-core/apis/nvidia-genai-image.yaml"),
    ),
];

/// Provider templates embedded at build time (`lexi-core/providers/`). A
/// provider entry with `template: <name>` is the template merged with the entry.
const EMBEDDED_TEMPLATES: &[(&str, &str)] = &[
    (
        "openai",
        include_str!("../../lexi-core/providers/openai.yaml"),
    ),
    (
        "anthropic",
        include_str!("../../lexi-core/providers/anthropic.yaml"),
    ),
    (
        "nvidia",
        include_str!("../../lexi-core/providers/nvidia.yaml"),
    ),
];

/// The names of the shipped provider templates.
pub fn template_names() -> Vec<&'static str> {
    EMBEDDED_TEMPLATES.iter().map(|(n, _)| *n).collect()
}

/// A shipped provider template.
pub fn provider_template(name: &str) -> Result<Value, HostError> {
    let (_, text) = EMBEDDED_TEMPLATES
        .iter()
        .find(|(n, _)| *n == name)
        .ok_or_else(|| {
            HostError::Config(format!(
                "unknown provider template {name:?} (shipped: {})",
                template_names().join(", ")
            ))
        })?;
    Ok(serde_yaml::from_str(text)?)
}

/// A provider entry with its `template:` (if any) applied.
fn resolve_template(entry: &Value) -> Result<Value, HostError> {
    match entry.get("template").and_then(Value::as_str) {
        Some(name) => {
            let mut merged = lexi_core::merge_provider_template(&provider_template(name)?, entry);
            merged["template"] = Value::from(name); // keep where it came from
                                                    // An entry's api_key list replaces the template's (never merged).
            if let Some(k) = entry.get("api_key") {
                merged["api_key"] = k.clone();
            }
            Ok(merged)
        }
        None => Ok(entry.clone()),
    }
}

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

/// Load `providers-config.yaml`, with each entry's `template:` applied (the
/// file itself is read and written raw by the provider writers below).
pub fn load_providers() -> Result<Value, HostError> {
    let path = lexi_home().join("providers").join("providers-config.yaml");
    let text = std::fs::read_to_string(&path)
        .map_err(|_| HostError::Config(format!("no providers-config at {}", path.display())))?;
    let mut doc: Value = serde_yaml::from_str(&text)?;
    if let Some(providers) = doc.get_mut("providers").and_then(Value::as_object_mut) {
        for entry in providers.values_mut() {
            *entry = resolve_template(entry)?;
        }
    }
    Ok(doc)
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

/// Effective config for the provider's catch-all binding (its first binding
/// without `for:`) — the API definition with the provider entry merged over it
/// (provider wins). See [`binding_config`] for a specific binding.
pub fn effective_config(provider_name: &str, provider_cfg: &Value) -> Result<Value, HostError> {
    let bindings = lexi_core::provider_bindings(provider_cfg);
    let Some(binding) = bindings
        .iter()
        .find(|b| b.get("for").is_none())
        .or(bindings.first())
    else {
        // No API reference: the provider may define its API inline.
        if provider_cfg.get("request").is_some_and(Value::is_object) {
            return Ok(provider_cfg.clone());
        }
        return Err(HostError::MissingApi(provider_name.to_string()));
    };
    binding_config(provider_cfg, binding)
}

/// Provider keys that describe the provider (its APIs and models, how to get a
/// key) rather than configure a request; not merged into an effective config.
const CLASSIFICATION_KEYS: &[&str] = &["apis", "api", "tag_rules", "template", "replace", "setup"];

/// Effective config for one binding: its API definition, then the provider
/// entry, then the binding's own keys (`url`, `auth`, `request`, `response`,
/// `default_model`, …).
pub fn binding_config(provider_cfg: &Value, binding: &Value) -> Result<Value, HostError> {
    let api_name = binding.get("api").and_then(Value::as_str).unwrap_or("");
    let api_def = load_api_def(api_name)?;
    let mut provider = provider_cfg.clone();
    if let Some(p) = provider.as_object_mut() {
        for k in CLASSIFICATION_KEYS {
            p.shift_remove(*k);
        }
    }
    let mut over = binding.clone();
    if let Some(b) = over.as_object_mut() {
        b.shift_remove("for");
    }
    let mut eff = deep_merge(&deep_merge(&api_def, &provider), &over);
    eff["api"] = Value::from(api_name);
    // An API's own api_key replaces the provider's (never merged).
    if let Some(k) = binding.get("api_key") {
        eff["api_key"] = k.clone();
    }
    Ok(eff)
}

/// The `api_key` sources in `cfg` — a provider entry, or a binding's or
/// model's effective config (whose own `api_key` replaces the provider's) — in
/// the order they're tried.
fn key_sources(cfg: &Value) -> Result<Vec<Source>, HostError> {
    keystore::parse_sources(cfg.get("api_key")).map_err(HostError::Config)
}

/// Look for the key `cfg`'s `api_key` sources lead to.
pub(crate) fn lookup_api_key(cfg: &Value) -> Result<Lookup, HostError> {
    Ok(keystore::lookup(&lexi_home(), &key_sources(cfg)?))
}

/// Resolve an API key from `cfg`'s `api_key` sources (a provider entry, or a
/// binding's/model's effective config — pass the effective config when there
/// is one, so an API's or model's own `api_key` applies). The first source
/// with a key wins; a read error (an unreadable file) stops the lookup and is
/// the error.
pub fn resolve_api_key(provider_name: &str, provider_cfg: &Value) -> Result<String, HostError> {
    let lookup = lookup_api_key(provider_cfg)?;
    if let Some(key) = lookup.key.clone() {
        return Ok(key);
    }
    if let Some((source, err)) = lookup.failure() {
        return Err(read_error(provider_name, source, err));
    }
    Err(HostError::MissingApiKey(if lookup.checked.is_empty() {
        format!("no API key for provider {provider_name:?}: its api_key in providers-config.yaml names no source")
    } else {
        format!(
            "no API key for provider {provider_name:?} (looked in {})",
            lookup.summary()
        )
    }))
}

fn read_error(provider_name: &str, source: &Source, err: &str) -> HostError {
    HostError::Config(format!(
        "couldn't read {provider_name}'s API key from {}: {err}",
        source.describe()
    ))
}

/// Which kind of source the key comes from — `"env"`, `"file"`, `"config"`
/// (a literal) — without revealing it; `None` when no source has one. Same
/// order as [`resolve_api_key`] (a read error is an error).
pub fn api_key_source(
    provider_name: &str,
    provider_cfg: &Value,
) -> Result<Option<&'static str>, HostError> {
    let lookup = lookup_api_key(provider_cfg)?;
    if let Some((source, err)) = lookup.failure() {
        return Err(read_error(provider_name, source, err));
    }
    Ok(lookup.used().map(Source::kind))
}

/// Where a saved key went (`set_api_key`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SavedKey {
    /// The file, as providers-config.yaml names it.
    pub location: String,
    /// Whether that file was added to providers-config.yaml.
    pub added: bool,
    /// The source the key is now read from — another one when an earlier
    /// source (e.g. a set environment variable) has a key.
    pub used: Option<String>,
}

impl SavedKey {
    pub fn to_value(&self) -> Value {
        serde_json::json!({
            "location": self.location,
            "added": self.added,
            "used": self.used,
        })
    }
}

/// The `api_key` list a key operation works on: the provider's, or (with
/// `api`) that API's own. `own` is false when the API has none and uses the
/// provider's.
struct KeyScope {
    doc: Map,
    entry: Map,
    api: Option<String>,
    sources: Vec<Source>,
    own: bool,
}

fn key_scope(home: &Path, provider: &str, api: Option<&str>) -> Result<KeyScope, HostError> {
    check_provider_name(provider)?;
    let mut doc = read_providers_doc(home)?;
    let entry = raw_entry(&mut doc, provider)?;
    let resolved = resolve_template(&Value::Object(entry.clone()))?;
    let (sources, own) = match api {
        None => (key_sources(&resolved)?, true),
        Some(a) => {
            let bindings = lexi_core::provider_bindings(&resolved);
            let binding = bindings.iter().find(|b| b["api"] == a).ok_or_else(|| {
                HostError::Invalid(format!(
                    "provider {provider:?} has no {a:?} API (it has: {})",
                    bindings
                        .iter()
                        .filter_map(|b| b["api"].as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ))
            })?;
            match binding.get("api_key") {
                Some(_) => (key_sources(binding)?, true),
                None => (Vec::new(), false),
            }
        }
    };
    Ok(KeyScope {
        doc,
        entry,
        api: api.map(String::from),
        sources,
        own,
    })
}

/// The key file Lexi saves `provider`'s key in (or its `api`'s own key).
fn key_file(provider: &str, api: Option<&str>) -> String {
    match api {
        Some(a) => format!("providers/{provider}/api-key-{a}"),
        None => format!("providers/{provider}/api-key"),
    }
}

/// Write `scope`'s list back into providers-config.yaml.
fn write_scope_sources(
    home: &Path,
    provider: &str,
    scope: KeyScope,
    sources: &[Source],
) -> Result<(), HostError> {
    match scope.api {
        None => {
            let (mut doc, mut entry) = (scope.doc, scope.entry);
            entry.insert("api_key".into(), keystore::sources_value(sources));
            providers_mut(&mut doc).insert(provider.to_string(), Value::Object(entry));
            write_yaml(&providers_config_path(home), &Value::Object(doc))
        }
        Some(api) => {
            let mut b = Map::new();
            b.insert("api".into(), Value::from(api));
            b.insert("api_key".into(), keystore::sources_value(sources));
            add_provider_api_in(home, provider, &b).map(|_| ())
        }
    }
}

/// Save `key` in the file `raw` names, readable only by you.
fn write_key_file(home: &Path, raw: &str, key: &str) -> Result<(), HostError> {
    let path = keystore::file_path(home, raw);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, format!("{key}\n"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

/// The source `provider`'s (or its `api`'s) key is read from now.
fn used_source(home: &Path, provider: &str, api: Option<&str>) -> Option<String> {
    let scope = key_scope(home, provider, api).ok()?;
    let sources = if scope.own {
        scope.sources
    } else {
        key_scope(home, provider, None).ok()?.sources
    };
    keystore::lookup(home, &sources)
        .used()
        .map(Source::describe)
}

/// What a key operation on `provider` (or its `api`) sees: the sources that
/// apply (the API's own, else the provider's), whether they're the API's
/// own, where a saved key would go (`adds`: that file isn't in the config
/// yet), and what looking through the sources found.
pub(crate) struct KeyPlan {
    pub(crate) sources: Vec<Source>,
    pub(crate) own: bool,
    pub(crate) save_to: Source,
    pub(crate) adds: bool,
    pub(crate) lookup: Lookup,
}

pub(crate) fn key_plan(provider: &str, api: Option<&str>) -> Result<KeyPlan, HostError> {
    let home = lexi_home();
    let scope = key_scope(&home, provider, api)?;
    let sources = if scope.own {
        scope.sources.clone()
    } else {
        key_scope(&home, provider, None)?.sources
    };
    let (save_to, adds) = match scope.sources.iter().find(|s| s.writable()) {
        Some(s) => (s.clone(), false),
        None => (Source::File(key_file(provider, api)), true),
    };
    let lookup = keystore::lookup(&home, &sources);
    Ok(KeyPlan {
        sources,
        own: scope.own,
        save_to,
        adds,
        lookup,
    })
}

/// Save an API key for `provider` — or, with `api`, a key for that API only —
/// in an owner-only file: the first file its `api_key` lists, else
/// `providers/<name>/api-key` (`api-key-<api>` for an API), which is added to
/// providers-config.yaml after the sources already there (a literal key in
/// the config is replaced).
pub fn set_api_key(
    provider_name: &str,
    api: Option<&str>,
    key: &str,
) -> Result<SavedKey, HostError> {
    set_api_key_in(&lexi_home(), provider_name, api, key)
}

fn set_api_key_in(
    home: &Path,
    provider_name: &str,
    api: Option<&str>,
    key: &str,
) -> Result<SavedKey, HostError> {
    let key = key.trim();
    if key.is_empty() {
        return Err(HostError::Config("API key cannot be empty".into()));
    }
    let scope = key_scope(home, provider_name, api)?;
    let (file, added) = match scope.sources.iter().find_map(|s| match s {
        Source::File(raw) => Some(raw.clone()),
        _ => None,
    }) {
        Some(raw) => (raw, false),
        None => (key_file(provider_name, api), true),
    };
    write_key_file(home, &file, key)?;
    if added {
        let mut list: Vec<Source> = scope
            .sources
            .iter()
            .filter(|s| !matches!(s, Source::Literal(_)))
            .cloned()
            .collect();
        list.push(Source::File(file.clone()));
        write_scope_sources(home, provider_name, scope, &list)?;
    }
    Ok(SavedKey {
        location: file,
        added,
        used: used_source(home, provider_name, api),
    })
}

fn uses_provider_key(provider: &str, api: &str) -> HostError {
    HostError::Invalid(format!(
        "the {api} API has no api_key of its own; it uses {provider}'s key (leave out the API)"
    ))
}

/// Delete `provider`'s (or its `api`'s) saved key — the first file its
/// `api_key` lists; providers-config.yaml is left as it is. A file outside
/// `$LEXI_HOME` is yours, not Lexi's, and is never deleted. Returns whether
/// there was one.
pub fn remove_api_key(provider_name: &str, api: Option<&str>) -> Result<bool, HostError> {
    remove_api_key_in(&lexi_home(), provider_name, api)
}

fn remove_api_key_in(
    home: &Path,
    provider_name: &str,
    api: Option<&str>,
) -> Result<bool, HostError> {
    let scope = key_scope(home, provider_name, api)?;
    if let (Some(a), false) = (api, scope.own) {
        return Err(uses_provider_key(provider_name, a));
    }
    let Some(raw) = scope.sources.iter().find_map(|s| match s {
        Source::File(raw) => Some(raw.clone()),
        _ => None,
    }) else {
        return Ok(false);
    };
    let path = keystore::file_path(home, &raw);
    if !path.starts_with(home) {
        return Err(HostError::Invalid(format!(
            "{} isn't in Lexi's folder, so Lexi won't delete it; remove it yourself or change providers-config.yaml",
            path.display()
        )));
    }
    if path.exists() {
        std::fs::remove_file(path)?;
        return Ok(true);
    }
    Ok(false)
}

/// One-time upgrades of an older config's `api_key` lists (best effort):
/// - an entry still in the old single form with a key file
///   (`providers/<name>/api-key`, which older versions read first) gets
///   `api_key: [{file: …}, …]`, so the same key keeps being used;
/// - a `{keychain: …}` entry (a development build kept keys in the OS
///   keychain) becomes the key file Lexi saves that key in — empty until
///   the key is saved again.
pub(crate) fn upgrade_key_sources(home: &Path) {
    let Ok(mut doc) = read_providers_doc(home) else {
        return;
    };
    let mut changed = false;
    for (name, entry) in providers_mut(&mut doc).iter_mut() {
        changed |= replace_keychain_sources(name, entry);
        let legacy = matches!(entry.get("api_key"), None | Some(Value::String(_)));
        let rel = format!("providers/{name}/api-key");
        if !legacy || !home.join(&rel).is_file() {
            continue;
        }
        let Ok(resolved) = resolve_template(entry) else {
            continue;
        };
        let mut list = vec![Source::File(rel)];
        list.extend(key_sources(&resolved).unwrap_or_default());
        if let Some(o) = entry.as_object_mut() {
            o.insert("api_key".into(), keystore::sources_value(&list));
            changed = true;
        }
    }
    if changed {
        let _ = write_yaml(&providers_config_path(home), &Value::Object(doc));
    }
}

/// In a raw provider entry, each `{keychain: …}` in its `api_key` (and its
/// APIs') becomes that scope's key file, listed once. Whether anything
/// changed.
fn replace_keychain_sources(provider: &str, entry: &mut Value) -> bool {
    let mut changed = false;
    let mut fix = |v: &mut Value, file: String| {
        let items: Vec<Value> = match v.get("api_key") {
            Some(Value::Array(a)) => a.clone(),
            Some(one) => vec![one.clone()],
            None => return,
        };
        if !items.iter().any(|i| i.get("keychain").is_some()) {
            return;
        }
        let mut out: Vec<Value> = Vec::new();
        for item in items {
            let item = if item.get("keychain").is_some() {
                serde_json::json!({ "file": file })
            } else {
                item
            };
            if !out.contains(&item) {
                out.push(item);
            }
        }
        v["api_key"] = Value::Array(out);
        changed = true;
    };
    fix(entry, key_file(provider, None));
    if let Some(apis) = entry.get_mut("apis").and_then(Value::as_array_mut) {
        for b in apis.iter_mut() {
            if let Some(api) = b.get("api").and_then(Value::as_str).map(String::from) {
                fix(b, key_file(provider, Some(&api)));
            }
        }
    }
    changed
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
    /// Display name (`name` in configured-models.yaml).
    pub name: Option<String>,
    pub description: Option<String>,
    pub default: bool,
    /// Aliases that resolve to this model id.
    pub aliases: Vec<String>,
    /// The model's full metadata entry (name, description, max_tokens, …).
    pub metadata: Value,
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
                name: meta.get("name").and_then(Value::as_str).map(str::to_string),
                description: meta
                    .get("description")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                default: default.as_deref() == Some(id.as_str()),
                aliases: alias_list,
                metadata: meta.clone(),
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
    if aliases.shift_remove(alias).is_none() {
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
        if models.shift_remove(model).is_none() {
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

/// The payload of a saved prompt: `name`, or the active prompt (`$$`) when `None`.
pub fn resolve_prompt(name: Option<&str>) -> Result<Value, HostError> {
    let name = name.unwrap_or(ACTIVE_PROMPT);
    match load_prompts()?.into_iter().find(|(n, _)| n == name) {
        Some((_, body)) => Ok(body),
        None if name == ACTIVE_PROMPT => Err(HostError::Prompt(
            "no active prompt; save one with `prompts set <text>`, name a saved prompt, \
             or pass text directly"
                .into(),
        )),
        None => Err(HostError::Prompt(format!("prompt {name:?} not found"))),
    }
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
/// The saved-prompt name that holds the active prompt.
pub const ACTIVE_PROMPT: &str = "$$";

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

/// The user-dir path for a custom API definition, rejecting names that contain
/// path separators or `..` (so a name can't escape the apis dir).
fn api_def_path(home: &Path, name: &str) -> Result<PathBuf, HostError> {
    let name = name.trim();
    if name.is_empty()
        || name.contains('/')
        || name.contains('\\')
        || name.split(['/', '\\']).any(|seg| seg == "..")
    {
        return Err(HostError::Config(format!(
            "invalid definition name {name:?}"
        )));
    }
    Ok(home.join("apis").join(format!("{name}.yaml")))
}

/// Set a definition's request path and/or response text path. A text path
/// switches extraction to simple mode, so the walked-mode keys are dropped.
fn apply_path_overrides(obj: &mut Map, path: Option<&str>, text_path: Option<&str>) {
    if let Some(p) = path {
        obj.entry("request")
            .or_insert_with(|| Value::Object(Map::new()));
        if let Some(req) = obj.get_mut("request").and_then(Value::as_object_mut) {
            req.insert("path".into(), Value::String(p.to_string()));
        }
    }
    if let Some(tp) = text_path {
        obj.entry("response")
            .or_insert_with(|| Value::Object(Map::new()));
        if let Some(resp) = obj.get_mut("response").and_then(Value::as_object_mut) {
            let te = resp
                .entry("text_extract")
                .or_insert_with(|| Value::Object(Map::new()));
            if let Some(te) = te.as_object_mut() {
                te.insert("text_path".into(), Value::String(tp.to_string()));
                for drop in [
                    "items_path",
                    "item_filter",
                    "content_path",
                    "content_filter",
                ] {
                    te.shift_remove(drop);
                }
            }
        }
    }
}

/// Create a new API definition. With `from`, deep-copies that definition as a
/// template (embedded or user); otherwise scaffolds a default chat-style schema.
/// `path`/`text_path` override the request path and response text path. Mirrors
/// the CLI's `api create`.
pub fn create_api_def(
    name: &str,
    from: Option<&str>,
    path: Option<&str>,
    text_path: Option<&str>,
) -> Result<(), HostError> {
    create_api_def_in(&lexi_home(), name, from, path, text_path)
}

fn create_api_def_in(
    home: &Path,
    name: &str,
    from: Option<&str>,
    path: Option<&str>,
    text_path: Option<&str>,
) -> Result<(), HostError> {
    let dest = api_def_path(home, name)?;
    let name = name.trim();

    let mut data = match from {
        Some(src) => load_api_def_from(home, src)?,
        None => default_api_scaffold(name, path, text_path),
    };
    let obj = data
        .as_object_mut()
        .ok_or_else(|| HostError::Config("definition must be a mapping".into()))?;
    obj.insert("name".into(), Value::String(name.to_string()));
    apply_path_overrides(obj, path, text_path);
    write_yaml(&dest, &data)
}

/// Load a definition to use as a `create --from` template: the embedded copy for
/// shipped names, else the user file.
fn load_api_def_from(home: &Path, name: &str) -> Result<Value, HostError> {
    if let Some(text) = embedded_api_text(name) {
        return Ok(serde_yaml::from_str(text)?);
    }
    let user = home.join("apis").join(format!("{name}.yaml"));
    if user.exists() {
        return Ok(serde_yaml::from_str(&std::fs::read_to_string(&user)?)?);
    }
    Err(HostError::Config(format!(
        "source API definition {name:?} not found"
    )))
}

/// The default chat-style definition scaffold the CLI seeds a new def with.
fn default_api_scaffold(name: &str, path: Option<&str>, text_path: Option<&str>) -> Value {
    serde_json::json!({
        "name": name,
        "description": "",
        "auth": { "header": "Authorization", "value_prefix": "Bearer " },
        "request": {
            "method": "POST",
            "path": path.unwrap_or("/chat/completions"),
            "body_schema": {
                "type": "object",
                "required": ["model", "messages"],
                "properties": {
                    "model": { "type": "string", "x-bind": { "from": "model" } },
                    "messages": {
                        "type": "array",
                        "x-render": { "template": [{ "role": "{{role}}", "content": "{{prompt}}" }] }
                    },
                    "temperature": {
                        "type": "number", "minimum": 0, "maximum": 2,
                        "x-cli": { "flag": "--temperature", "alias": "-t" }
                    },
                    "max_tokens": {
                        "type": "integer", "minimum": 1,
                        "x-cli": { "flag": "--max-tokens", "alias": "-m" }
                    }
                }
            }
        },
        "response": {
            "text_extract": { "text_path": text_path.unwrap_or("choices[0].message.content") }
        }
    })
}

/// Save raw YAML for a (custom) API definition, then validate it. Errors if the
/// YAML doesn't parse; otherwise writes the file and returns the validation
/// errors (empty = valid). Mirrors the CLI's save-then-validate.
pub fn save_api_raw(name: &str, yaml: &str) -> Result<Vec<String>, HostError> {
    save_api_raw_in(&lexi_home(), name, yaml)
}

fn save_api_raw_in(home: &Path, name: &str, yaml: &str) -> Result<Vec<String>, HostError> {
    let dest = api_def_path(home, name)?;
    let parsed: Value =
        serde_yaml::from_str(yaml).map_err(|e| HostError::Config(format!("Invalid YAML: {e}")))?;
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&dest, yaml)?;
    Ok(lexi_core::validate_definition(&parsed))
}

/// Build the editor live-preview for a definition given either its raw `yaml`
/// (which takes priority) or its `name`. Unparseable/non-object YAML yields a
/// `{valid:false, errors:[...]}` result rather than an error, matching the CLI.
pub fn preview_api_def(
    name: Option<&str>,
    yaml: Option<&str>,
    prompt: &str,
    model: &str,
    role: &str,
) -> Result<Value, HostError> {
    let def = if let Some(y) = yaml {
        match serde_yaml::from_str::<Value>(y) {
            Ok(v) if v.is_object() => v,
            Ok(_) => {
                return Ok(serde_json::json!({
                    "valid": false,
                    "errors": ["YAML top level must be an object"],
                }))
            }
            Err(e) => {
                return Ok(serde_json::json!({
                    "valid": false,
                    "errors": [format!("YAML parse error: {e}")],
                }))
            }
        }
    } else if let Some(n) = name {
        load_api_def(n)?
    } else {
        return Err(HostError::Config("Provide either yaml or name".into()));
    };
    Ok(lexi_core::preview_definition(&def, prompt, model, role))
}

/// Validate a stored API definition, returning `(valid, errors)`.
pub fn validate_api_def(name: &str) -> Result<(bool, Vec<String>), HostError> {
    let def = load_api_def(name)?;
    let errors = lexi_core::validate_definition(&def);
    Ok((errors.is_empty(), errors))
}

/// Delete a custom API definition. Errors if no user file exists (shipped
/// definitions have no user file to remove).
pub fn delete_api_def(name: &str) -> Result<(), HostError> {
    delete_api_def_in(&lexi_home(), name)
}

fn delete_api_def_in(home: &Path, name: &str) -> Result<(), HostError> {
    let path = api_def_path(home, name)?;
    if !path.exists() {
        return Err(HostError::Config(format!(
            "API definition {name:?} not found"
        )));
    }
    std::fs::remove_file(&path)?;
    Ok(())
}

/// Remove a key from `config.json`; returns whether it was present.
pub fn delete_config(key: &str) -> Result<bool, HostError> {
    delete_config_in(&lexi_home(), key)
}

fn delete_config_in(home: &Path, key: &str) -> Result<bool, HostError> {
    let mut cfg = match load_config_in(home)? {
        Value::Object(m) => m,
        _ => Map::new(),
    };
    if cfg.shift_remove(key).is_none() {
        return Ok(false);
    }
    write_json(&home.join("config.json"), &Value::Object(cfg))?;
    Ok(true)
}

fn providers_config_path(home: &Path) -> PathBuf {
    home.join("providers").join("providers-config.yaml")
}

/// Read providers-config.yaml as a mutable mapping (empty if absent), with a
/// `providers` mapping guaranteed present.
fn read_providers_doc(home: &Path) -> Result<Map, HostError> {
    let path = providers_config_path(home);
    let mut doc = if path.exists() {
        match serde_yaml::from_str::<Value>(&std::fs::read_to_string(&path)?)? {
            Value::Object(m) => m,
            Value::Null => Map::new(),
            _ => {
                return Err(HostError::Config(
                    "providers-config.yaml must be a mapping".into(),
                ))
            }
        }
    } else {
        Map::new()
    };
    if !doc.get("providers").is_some_and(Value::is_object) {
        doc.insert("providers".into(), Value::Object(Map::new()));
    }
    Ok(doc)
}

fn providers_mut(doc: &mut Map) -> &mut Map {
    doc.get_mut("providers")
        .and_then(Value::as_object_mut)
        .expect("read_providers_doc guarantees a providers mapping")
}

/// A provider name usable as a directory: non-empty, no separators or `..`.
fn check_provider_name(name: &str) -> Result<(), HostError> {
    if name.trim().is_empty() || name.contains(['/', '\\']) || name == ".." || name == "." {
        return Err(HostError::Config(format!("invalid provider name {name:?}")));
    }
    Ok(())
}

/// Add a provider entry to providers-config.yaml. Errors if it exists.
pub fn add_provider(name: &str, provider_cfg: &Map) -> Result<(), HostError> {
    add_provider_in(&lexi_home(), name, provider_cfg)
}

fn add_provider_in(home: &Path, name: &str, provider_cfg: &Map) -> Result<(), HostError> {
    check_provider_name(name)?;
    let mut doc = read_providers_doc(home)?;
    let providers = providers_mut(&mut doc);
    if providers.contains_key(name) {
        return Err(HostError::Config(format!(
            "provider {name:?} already exists"
        )));
    }
    providers.insert(name.to_string(), Value::Object(provider_cfg.clone()));
    write_yaml(&providers_config_path(home), &Value::Object(doc))
}

/// Rename a provider: its config entry (position kept) and its
/// `providers/<name>/` directory (key files in it follow, and the config's
/// `api_key` lists point at their new place).
pub fn rename_provider(old: &str, new: &str) -> Result<(), HostError> {
    rename_provider_in(&lexi_home(), old, new)
}

fn rename_provider_in(home: &Path, old: &str, new: &str) -> Result<(), HostError> {
    check_provider_name(new)?;
    let mut doc = read_providers_doc(home)?;
    let providers = providers_mut(&mut doc);
    if !providers.contains_key(old) {
        return Err(HostError::UnknownProvider(old.to_string()));
    }
    if providers.contains_key(new) {
        return Err(HostError::Config(format!(
            "provider {new:?} already exists"
        )));
    }
    let renamed: Map = std::mem::take(providers)
        .into_iter()
        .map(|(k, v)| {
            if k == old {
                (new.to_string(), v)
            } else {
                (k, v)
            }
        })
        .collect();
    *providers = renamed;
    if let Some(entry) = providers.get_mut(new) {
        let old_dir = format!("providers/{old}/");
        rewrite_entry_sources(entry, |list| {
            list.into_iter()
                .map(|s| match s {
                    Source::File(p) => match p.strip_prefix(&old_dir) {
                        Some(rest) => Source::File(format!("providers/{new}/{rest}")),
                        None => Source::File(p),
                    },
                    other => other,
                })
                .collect()
        });
    }
    write_yaml(&providers_config_path(home), &Value::Object(doc))?;
    let old_dir = home.join("providers").join(old);
    if old_dir.exists() {
        std::fs::rename(&old_dir, home.join("providers").join(new))?;
    }
    Ok(())
}

/// Rewrite each `api_key` list written in a raw provider entry (its own and
/// its APIs').
fn rewrite_entry_sources(entry: &mut Value, mut f: impl FnMut(Vec<Source>) -> Vec<Source>) {
    let mut apply = |v: &mut Value| {
        if let Some(k) = v.get("api_key") {
            if let Ok(list) = keystore::parse_sources(Some(k)) {
                let before = list.clone();
                let after = f(list);
                if after != before {
                    v["api_key"] = keystore::sources_value(&after);
                }
            }
        }
    };
    apply(entry);
    if let Some(apis) = entry.get_mut("apis").and_then(Value::as_array_mut) {
        for b in apis.iter_mut().filter(|b| b.is_object()) {
            apply(b);
        }
    }
}

/// Remove a provider: its config entry and its `providers/<name>/` directory
/// (models, aliases, default model, key files).
pub fn remove_provider(name: &str) -> Result<(), HostError> {
    remove_provider_in(&lexi_home(), name)
}

fn remove_provider_in(home: &Path, name: &str) -> Result<(), HostError> {
    check_provider_name(name)?;
    let mut doc = read_providers_doc(home)?;
    if providers_mut(&mut doc).shift_remove(name).is_none() {
        return Err(HostError::UnknownProvider(name.to_string()));
    }
    write_yaml(&providers_config_path(home), &Value::Object(doc))?;
    let dir = home.join("providers").join(name);
    if dir.exists() {
        std::fs::remove_dir_all(dir)?;
    }
    Ok(())
}

/// Edit a custom API definition in place: request path, response text path,
/// and `response.metadata` entries (`key -> path`). Shipped definitions are
/// read-only (the runtime always loads their embedded copy) — copy one with
/// [`create_api_def`] `from` first.
pub fn edit_api_def(
    name: &str,
    path: Option<&str>,
    text_path: Option<&str>,
    set_meta: &[(String, String)],
) -> Result<(), HostError> {
    edit_api_def_in(&lexi_home(), name, path, text_path, set_meta)
}

fn edit_api_def_in(
    home: &Path,
    name: &str,
    path: Option<&str>,
    text_path: Option<&str>,
    set_meta: &[(String, String)],
) -> Result<(), HostError> {
    if embedded_api_text(name).is_some() {
        return Err(HostError::Config(format!(
            "{name:?} is a shipped definition and can't be edited; copy it first \
             (create a new definition from it) and edit the copy"
        )));
    }
    let dest = api_def_path(home, name)?;
    if !dest.exists() {
        return Err(HostError::UnknownApi(name.to_string()));
    }
    let mut data: Value = serde_yaml::from_str(&std::fs::read_to_string(&dest)?)?;
    let obj = data
        .as_object_mut()
        .ok_or_else(|| HostError::Config("definition must be a mapping".into()))?;
    apply_path_overrides(obj, path, text_path);
    if !set_meta.is_empty() {
        let resp = obj
            .entry("response")
            .or_insert_with(|| Value::Object(Map::new()));
        if let Some(resp) = resp.as_object_mut() {
            let meta = resp
                .entry("metadata")
                .or_insert_with(|| Value::Object(Map::new()));
            if let Some(meta) = meta.as_object_mut() {
                for (k, v) in set_meta {
                    meta.insert(k.clone(), Value::String(v.clone()));
                }
            }
        }
    }
    write_yaml(&dest, &data)
}

// ---------------------------------------------------------------------------
// Templates and bindings: `providers upgrade`, `providers api …`.
// ---------------------------------------------------------------------------

/// The providers-config.yaml shipped before templates; `upgrade_provider`
/// drops values still at these defaults.
const LEGACY_DEFAULTS: &str =
    include_str!("../../lexi-core/providers/legacy-providers-config.yaml");

fn legacy_entry(template: &str) -> Value {
    serde_yaml::from_str::<Value>(LEGACY_DEFAULTS)
        .ok()
        .and_then(|d| d.get("providers")?.get(template).cloned())
        .unwrap_or(Value::Null)
}

fn raw_entry(doc: &mut Map, name: &str) -> Result<Map, HostError> {
    providers_mut(doc)
        .get(name)
        .and_then(Value::as_object)
        .cloned()
        .ok_or_else(|| HostError::UnknownProvider(name.to_string()))
}

/// Copy providers-config.yaml aside (`.bak-<unix secs>[-n]`) before a rewrite.
fn backup_providers_config(home: &Path) -> Result<PathBuf, HostError> {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let src = providers_config_path(home);
    // Never reuse a name: several rewrites in one second must each keep theirs
    // (the first holds the untouched original).
    let dest = (1..)
        .map(|n| match n {
            1 => src.with_file_name(format!("providers-config.yaml.bak-{secs}")),
            n => src.with_file_name(format!("providers-config.yaml.bak-{secs}-{n}")),
        })
        .find(|p| !p.exists())
        .expect("an unused backup name");
    std::fs::copy(&src, &dest)?;
    Ok(dest)
}

/// Convert a provider entry to the template form: `template: <name>` plus only
/// the values that differ from both the template and the old shipped
/// defaults. A custom single `api` / `default_model` become a binding (so the
/// template's own bindings don't hide them). `template` defaults to the
/// provider's name. With `apply`, backs up and rewrites providers-config.yaml;
/// otherwise reports what would change. Returns `{provider, template, changed,
/// before, after, dropped, kept, backup?}`.
pub fn upgrade_provider(
    name: &str,
    template: Option<&str>,
    apply: bool,
) -> Result<Value, HostError> {
    let home = lexi_home();
    let mut doc = read_providers_doc(&home)?;
    let entry = raw_entry(&mut doc, name)?;
    if let Some(t) = entry.get("template").and_then(Value::as_str) {
        if template.is_none_or(|want| want == t) {
            return Ok(serde_json::json!({
                "provider": name, "template": t, "changed": false,
                "before": entry, "after": entry, "dropped": [], "kept": [],
            }));
        }
    }
    let tname = template.unwrap_or(name);
    let tmpl = provider_template(tname)?;
    let legacy = legacy_entry(tname);
    let resolved = lexi_core::merge_provider_template(&tmpl, &serde_json::json!({}));
    let catch_all_api = lexi_core::provider_bindings(&resolved)
        .into_iter()
        .find(|b| b.get("for").is_none())
        .and_then(|b| b["api"].as_str().map(String::from));

    let mut after = Map::new();
    after.insert("template".into(), Value::from(tname));
    let (mut dropped, mut kept) = (Vec::new(), Vec::new());
    let mut user_bindings: Vec<Value> = Vec::new();
    let custom_api = entry
        .get("api")
        .and_then(Value::as_str)
        .filter(|a| Some(*a) != legacy.get("api").and_then(Value::as_str))
        .filter(|a| Some(*a) != catch_all_api.as_deref())
        .map(String::from);
    for (k, v) in &entry {
        if k == "template" {
            continue;
        }
        let at_default = legacy.get(k) == Some(v) || resolved.get(k) == Some(v);
        match k.as_str() {
            _ if at_default => dropped.push(k.clone()),
            "api" => {
                // A custom API becomes the catch-all binding (listed first).
                user_bindings.insert(0, serde_json::json!({ "api": v }));
                kept.push(k.clone());
            }
            "default_model" => {
                let api = custom_api
                    .clone()
                    .or(catch_all_api.clone())
                    .unwrap_or_default();
                match user_bindings.iter_mut().find(|b| b["api"] == api.as_str()) {
                    Some(b) => b["default_model"] = v.clone(),
                    None => {
                        user_bindings.push(serde_json::json!({ "api": api, "default_model": v }))
                    }
                }
                kept.push(k.clone());
            }
            _ => {
                after.insert(k.clone(), v.clone());
                kept.push(k.clone());
            }
        }
    }
    if !user_bindings.is_empty() {
        after.insert("apis".into(), Value::Array(user_bindings));
    }
    let mut out = serde_json::json!({
        "provider": name, "template": tname, "changed": true,
        "before": entry, "after": after, "dropped": dropped, "kept": kept,
    });
    if apply {
        out["backup"] = Value::from(backup_providers_config(&home)?.display().to_string());
        providers_mut(&mut doc).insert(name.to_string(), Value::Object(after));
        write_yaml(&providers_config_path(&home), &Value::Object(doc))?;
    }
    Ok(out)
}

/// A provider's bindings with where each comes from:
/// `[{api, for?, default_model?, url?, enabled, source: "template"|"user"|"config"}]`.
pub fn provider_apis(name: &str) -> Result<Vec<Value>, HostError> {
    let home = lexi_home();
    let mut doc = read_providers_doc(&home)?;
    let entry = Value::Object(raw_entry(&mut doc, name)?);
    let resolved = resolve_template(&entry)?;
    let user: Vec<String> = entry
        .get("apis")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|b| {
            b.get("api")
                .or(Some(b))
                .and_then(Value::as_str)
                .map(String::from)
        })
        .collect();
    let templated = entry.get("template").is_some();
    let all: Vec<Value> = resolved
        .get("apis")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_else(|| lexi_core::provider_bindings(&resolved));
    Ok(all
        .into_iter()
        .map(|b| {
            let mut b = match b {
                Value::String(s) => serde_json::json!({ "api": s }),
                other => other,
            };
            let api = b["api"].as_str().unwrap_or("").to_string();
            let source = if !templated {
                "config"
            } else if user.contains(&api) {
                "user"
            } else {
                "template"
            };
            let enabled = b.get("enabled") != Some(&Value::Bool(false));
            if let Some(o) = b.as_object_mut() {
                o.insert("enabled".into(), Value::Bool(enabled));
                o.insert("source".into(), Value::from(source));
            }
            b
        })
        .collect())
}

/// Add (or update) a binding on a provider: `binding` is `{api, for?,
/// default_model?, url?}`. An older single-`api` entry is converted to an
/// `apis` list first. Returns true if the binding already existed.
pub fn add_provider_api(name: &str, binding: &Map) -> Result<bool, HostError> {
    add_provider_api_in(&lexi_home(), name, binding)
}

fn add_provider_api_in(home: &Path, name: &str, binding: &Map) -> Result<bool, HostError> {
    let api = binding
        .get("api")
        .and_then(Value::as_str)
        .ok_or_else(|| HostError::Invalid("a binding needs an `api`".into()))?
        .to_string();
    load_api_def(&api)?;
    for t in binding
        .get("for")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        lexi_core::validate_tag(t).map_err(HostError::Invalid)?;
    }
    let mut doc = read_providers_doc(home)?;
    let mut entry = raw_entry(&mut doc, name)?;
    let mut list: Vec<Value> = match entry.get("apis").and_then(Value::as_array) {
        Some(l) => l.clone(),
        None if entry.get("template").is_none() => {
            // Older entry: its single `api` becomes the first binding.
            let b = lexi_core::provider_bindings(&Value::Object(entry.clone()));
            entry.shift_remove("api");
            b
        }
        None => Vec::new(),
    };
    let existed = match list
        .iter_mut()
        .find(|b| b.get("api").or(Some(*b)).and_then(Value::as_str) == Some(api.as_str()))
    {
        Some(b) => {
            if b.is_string() {
                *b = serde_json::json!({ "api": api });
            }
            if let Some(o) = b.as_object_mut() {
                o.shift_remove("enabled");
                for (k, v) in binding {
                    o.insert(k.clone(), v.clone());
                }
            }
            true
        }
        None => {
            list.push(Value::Object(binding.clone()));
            false
        }
    };
    // A binding the template has counts as existing too; and a user entry
    // that only names it (re-enabling it) is redundant — drop it.
    let in_template = entry
        .get("template")
        .and_then(Value::as_str)
        .map(provider_template)
        .transpose()?
        .is_some_and(|t| {
            lexi_core::provider_bindings(&t)
                .iter()
                .any(|b| b["api"] == api.as_str())
        });
    let existed = existed || in_template;
    if in_template {
        list.retain(|b| {
            !(b.get("api").and_then(Value::as_str) == Some(api.as_str())
                && b.as_object().is_some_and(|o| o.len() == 1))
        });
    }
    if list.is_empty() {
        entry.shift_remove("apis");
    } else {
        entry.insert("apis".into(), Value::Array(list));
    }
    providers_mut(&mut doc).insert(name.to_string(), Value::Object(entry));
    write_yaml(&providers_config_path(home), &Value::Object(doc))?;
    Ok(existed)
}

/// Remove a binding: one you added is deleted; one the template provides is
/// turned off (`enabled: false`). Refuses to leave the provider with no API.
pub fn remove_provider_api(name: &str, api: &str) -> Result<(), HostError> {
    let home = lexi_home();
    let mut doc = read_providers_doc(&home)?;
    let mut entry = raw_entry(&mut doc, name)?;
    let resolved = resolve_template(&Value::Object(entry.clone()))?;
    let active: Vec<String> = lexi_core::provider_bindings(&resolved)
        .into_iter()
        .filter_map(|b| b["api"].as_str().map(String::from))
        .collect();
    if !active.iter().any(|a| a == api) {
        return Err(HostError::Invalid(format!(
            "provider {name:?} has no {api:?} API (it has: {})",
            active.join(", ")
        )));
    }
    if active.len() == 1 {
        return Err(HostError::Invalid(format!(
            "{api:?} is {name:?}'s only API; add another first, or remove the provider"
        )));
    }
    let in_template = entry
        .get("template")
        .and_then(Value::as_str)
        .map(provider_template)
        .transpose()?
        .is_some_and(|t| {
            lexi_core::provider_bindings(&t)
                .iter()
                .any(|b| b["api"] == api)
        });
    let mut list: Vec<Value> = match entry.get("apis").and_then(Value::as_array) {
        Some(l) => l.clone(),
        None => {
            let b = lexi_core::provider_bindings(&Value::Object(entry.clone()));
            entry.shift_remove("api");
            b
        }
    };
    list.retain(|b| b.get("api").or(Some(b)).and_then(Value::as_str) != Some(api));
    if in_template {
        list.push(serde_json::json!({ "api": api, "enabled": false }));
    }
    entry.insert("apis".into(), Value::Array(list));
    providers_mut(&mut doc).insert(name.to_string(), Value::Object(entry));
    write_yaml(&providers_config_path(&home), &Value::Object(doc))
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
    fn create_api_def_scaffolds_valid_definition() {
        let dir = tempfile::tempdir().unwrap();
        create_api_def_in(dir.path(), "mine", None, None, None).unwrap();
        let raw = load_api_raw_in(dir.path(), "mine").unwrap();
        let def: Value = serde_yaml::from_str(&raw).unwrap();
        assert_eq!(def["name"], "mine");
        // The scaffold is itself a valid definition.
        assert!(lexi_core::validate_definition(&def).is_empty());
    }

    #[test]
    fn create_api_def_from_copies_source_and_applies_overrides() {
        let dir = tempfile::tempdir().unwrap();
        create_api_def_in(
            dir.path(),
            "copy",
            Some("messages"),
            Some("/v2/x"),
            Some("a.b"),
        )
        .unwrap();
        let def: Value =
            serde_yaml::from_str(&load_api_raw_in(dir.path(), "copy").unwrap()).unwrap();
        assert_eq!(def["name"], "copy");
        assert_eq!(def["request"]["path"], "/v2/x");
        assert_eq!(def["response"]["text_extract"]["text_path"], "a.b");
    }

    #[test]
    fn save_api_raw_writes_and_validates() {
        let dir = tempfile::tempdir().unwrap();
        // Invalid YAML is rejected before any write.
        assert!(save_api_raw_in(dir.path(), "bad", "a: [unclosed").is_err());
        assert!(!dir.path().join("apis").join("bad.yaml").exists());

        // Valid YAML writes; incomplete definition returns errors (still saved).
        let errors = save_api_raw_in(dir.path(), "wip", "name: wip\n").unwrap();
        assert!(!errors.is_empty());
        assert!(dir.path().join("apis").join("wip.yaml").exists());

        // A complete definition validates clean.
        let good = "name: ok\nrequest:\n  path: /x\n  body_schema:\n    type: object\nresponse:\n  text_extract:\n    text_path: a.b\n";
        assert!(save_api_raw_in(dir.path(), "ok", good).unwrap().is_empty());
    }

    #[test]
    fn delete_api_def_removes_user_file_or_errors() {
        let dir = tempfile::tempdir().unwrap();
        assert!(delete_api_def_in(dir.path(), "nope").is_err());
        save_api_raw_in(dir.path(), "temp", "name: temp\n").unwrap();
        delete_api_def_in(dir.path(), "temp").unwrap();
        assert!(!dir.path().join("apis").join("temp.yaml").exists());
    }

    #[test]
    fn api_def_name_guard_rejects_traversal() {
        let dir = tempfile::tempdir().unwrap();
        assert!(create_api_def_in(dir.path(), "../evil", None, None, None).is_err());
        assert!(save_api_raw_in(dir.path(), "a/b", "name: x\n").is_err());
    }

    #[test]
    fn preview_api_def_handles_yaml_name_and_bad_input() {
        // Raw YAML of a valid scaffold composes a body.
        let good = "name: ok\nrequest:\n  path: /x\n  body_schema:\n    type: object\nresponse:\n  text_extract:\n    text_path: a.b\n";
        let out = preview_api_def(None, Some(good), "hi", "m", "user").unwrap();
        assert_eq!(out["valid"], true);
        assert!(out.get("body").is_some());

        // By name: a shipped definition resolves via the embedded copy.
        let out = preview_api_def(Some("messages"), None, "hi", "m", "user").unwrap();
        assert_eq!(out["name"], "messages");

        // Unparseable YAML -> valid:false result, not an error.
        let out = preview_api_def(None, Some("a: [bad"), "hi", "m", "user").unwrap();
        assert_eq!(out["valid"], false);

        // Neither name nor yaml -> error.
        assert!(preview_api_def(None, None, "hi", "m", "user").is_err());
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

    #[test]
    fn delete_config_removes_only_present_keys() {
        let dir = tempfile::tempdir().unwrap();
        let mut m = Map::new();
        m.insert("a".into(), json!("1"));
        m.insert("b".into(), json!("2"));
        set_config_in(dir.path(), &m).unwrap();
        assert!(delete_config_in(dir.path(), "a").unwrap());
        assert!(!delete_config_in(dir.path(), "a").unwrap());
        assert_eq!(load_config_in(dir.path()).unwrap(), json!({"b": "2"}));
    }

    #[test]
    fn provider_add_rename_remove() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        std::fs::create_dir_all(home.join("providers")).unwrap();
        std::fs::write(
            providers_config_path(home),
            "providers:\n  first:\n    url: u1\n  last:\n    url: u3\ndefaults:\n  max_tokens: 1\n",
        )
        .unwrap();
        let mut cfg = Map::new();
        cfg.insert("url".into(), json!("u2"));
        add_provider_in(home, "mid", &cfg).unwrap();
        assert!(add_provider_in(home, "mid", &cfg).is_err());
        assert!(add_provider_in(home, "../x", &cfg).is_err());

        set_api_key_in(home, "mid", None, "mid-key").unwrap();
        rename_provider_in(home, "mid", "middle").unwrap();
        // Key files Lexi saved follow the provider to its new name.
        assert!(home.join("providers/middle/api-key").exists());
        assert_eq!(
            entry_key(home, "middle"),
            json!([{"file": "providers/middle/api-key"}])
        );
        rename_provider_in(home, "first", "alpha").unwrap();
        assert!(rename_provider_in(home, "nope", "x").is_err());
        assert!(rename_provider_in(home, "alpha", "last").is_err());

        let doc = read_providers_doc(home).unwrap();
        let names: Vec<&String> = doc["providers"].as_object().unwrap().keys().collect();
        assert_eq!(names, ["alpha", "last", "middle"]);
        assert_eq!(doc["defaults"]["max_tokens"], 1); // other sections kept

        remove_provider_in(home, "middle").unwrap();
        assert!(!home.join("providers/middle").exists());
        assert!(remove_provider_in(home, "middle").is_err());
    }

    #[test]
    fn edit_api_def_patches_custom_and_refuses_shipped() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        create_api_def_in(home, "mine", Some("messages"), None, None).unwrap();
        edit_api_def_in(
            home,
            "mine",
            Some("/v2/messages"),
            Some("content[0].text"),
            &[("stop".into(), "stop_reason".into())],
        )
        .unwrap();
        let def = load_api_def_from(home, "mine").unwrap();
        assert_eq!(def["request"]["path"], "/v2/messages");
        assert_eq!(
            def["response"]["text_extract"]["text_path"],
            "content[0].text"
        );
        assert!(def["response"]["text_extract"].get("items_path").is_none());
        assert_eq!(def["response"]["metadata"]["stop"], "stop_reason");

        assert!(edit_api_def_in(home, "messages", Some("/x"), None, &[]).is_err());
        assert!(edit_api_def_in(home, "missing", Some("/x"), None, &[]).is_err());
    }

    fn write_providers(home: &Path, yaml: &str) {
        std::fs::create_dir_all(home.join("providers")).unwrap();
        std::fs::write(providers_config_path(home), yaml).unwrap();
    }

    fn entry_key(home: &Path, provider: &str) -> Value {
        read_providers_doc(home).unwrap()["providers"][provider]["api_key"].clone()
    }

    fn lookup_in(home: &Path, cfg: &Value) -> Lookup {
        keystore::lookup(home, &key_sources(cfg).unwrap())
    }

    #[test]
    fn key_sources_are_tried_in_order_and_only_a_missing_key_moves_on() {
        use crate::keystore::Outcome;
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        std::fs::write(home.join("empty.txt"), "\n").unwrap();
        std::fs::write(home.join("k.txt"), "from-file\n").unwrap();
        std::fs::write(home.join("later.txt"), "later\n").unwrap();
        let cfg = json!({"api_key": [
            "${LEXI_TEST_SURELY_UNSET_VAR}",
            {"file": "nothing-here"},
            {"file": "empty.txt"},
            {"file": "k.txt"},
            {"file": "later.txt"},
        ]});
        let found = lookup_in(home, &cfg);
        assert_eq!(found.key.as_deref(), Some("from-file"));
        let states: Vec<&str> = found.checked.iter().map(|(_, o)| o.state()).collect();
        assert_eq!(
            states,
            ["missing", "missing", "missing", "used", "not checked"]
        );
        assert_eq!(
            found.summary(),
            "$LEXI_TEST_SURELY_UNSET_VAR (not set), nothing-here (no file), empty.txt (empty), k.txt, later.txt"
        );

        // A set variable comes first when it's listed first.
        let env_first = json!({"api_key": [{"env": "PATH"}, {"file": "k.txt"}]});
        assert_eq!(
            lookup_in(home, &env_first).used().map(Source::kind),
            Some("env")
        );

        // An unreadable file stops the lookup — the file after it isn't used.
        std::fs::create_dir(home.join("a-folder")).unwrap();
        let unreadable = json!({"api_key": [{"file": "a-folder"}, {"file": "k.txt"}]});
        let found = lookup_in(home, &unreadable);
        assert_eq!(found.key, None);
        assert!(found.failure().is_some());
        assert_eq!(found.checked[1].1, Outcome::NotChecked);

        // The older forms still read; bad entries are config errors, and a
        // keychain entry says what to use instead.
        assert_eq!(
            lookup_in(home, &json!({"api_key": "literal"}))
                .key
                .as_deref(),
            Some("literal")
        );
        assert!(key_sources(&json!({"api_key": {"vault": "x"}})).is_err());
        assert!(key_sources(&json!({"api_key": [{"env": ""}]})).is_err());
        assert!(key_sources(&json!({})).unwrap().is_empty());
        let Err(e) = key_sources(&json!({"api_key": [{"keychain": "openai"}]})) else {
            panic!("a keychain entry is an error");
        };
        assert!(
            e.to_string().contains("doesn't use the system keychain"),
            "{e}"
        );
    }

    #[test]
    fn saving_uses_the_listed_file_or_adds_one() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        write_providers(
            home,
            "providers:\n  p:\n    url: u\n    api_key: ${LEXI_TEST_SURELY_UNSET_VAR}\n  q:\n    url: u\n    api_key: sk-literal\n  m:\n    url: u\n    api_key: [{file: my-keys/m.key}]\n",
        );

        // No file listed: the provider's key file is added after the variable.
        let saved = set_api_key_in(home, "p", None, " k1 ").unwrap();
        assert_eq!(
            (saved.location.as_str(), saved.added),
            ("providers/p/api-key", true)
        );
        assert_eq!(saved.used.as_deref(), Some("providers/p/api-key"));
        assert_eq!(
            entry_key(home, "p"),
            json!([{"env": "LEXI_TEST_SURELY_UNSET_VAR"}, {"file": "providers/p/api-key"}])
        );
        assert_eq!(
            std::fs::read_to_string(home.join("providers/p/api-key")).unwrap(),
            "k1\n"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(home.join("providers/p/api-key"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        // Saving again writes the same file; the config doesn't change.
        assert!(!set_api_key_in(home, "p", None, "k2").unwrap().added);

        // A literal key in the config is replaced; a listed file is used.
        set_api_key_in(home, "q", None, "k3").unwrap();
        assert_eq!(
            entry_key(home, "q"),
            json!([{"file": "providers/q/api-key"}])
        );
        let mine = set_api_key_in(home, "m", None, "k4").unwrap();
        assert_eq!(
            (mine.location.as_str(), mine.added),
            ("my-keys/m.key", false)
        );
        assert!(home.join("my-keys/m.key").exists());
        assert!(set_api_key_in(home, "q", None, "  ").is_err());
        assert!(set_api_key_in(home, "nope", None, "k").is_err());

        // Remove deletes the saved key and leaves the config as it is.
        assert!(remove_api_key_in(home, "p", None).unwrap());
        assert!(!remove_api_key_in(home, "p", None).unwrap());
        assert_eq!(entry_key(home, "p").as_array().unwrap().len(), 2);

        // A file outside Lexi's folder is never deleted.
        let outside = tempfile::NamedTempFile::new().unwrap();
        write_providers(
            home,
            &format!(
                "providers:\n  r:\n    url: u\n    api_key: {{file: {}}}\n",
                outside.path().display()
            ),
        );
        assert!(remove_api_key_in(home, "r", None).is_err());
        assert!(outside.path().exists());
    }

    #[test]
    fn an_api_can_have_its_own_key() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        write_providers(
            home,
            "providers:\n  p:\n    url: u\n    api_key: {file: main.key}\n    apis:\n      - api: chat-completions\n      - api: embeddings\n        for: [kind:embedding]\n",
        );
        std::fs::write(home.join("main.key"), "main-key\n").unwrap();

        // Without a list of its own an API uses the provider's key.
        assert!(!key_scope(home, "p", Some("embeddings")).unwrap().own);
        assert!(remove_api_key_in(home, "p", Some("embeddings")).is_err());
        assert!(key_scope(home, "p", Some("nope")).is_err());

        let saved = set_api_key_in(home, "p", Some("embeddings"), "emb-key").unwrap();
        assert!(saved.added);
        assert_eq!(saved.location, "providers/p/api-key-embeddings");
        let doc = read_providers_doc(home).unwrap();
        let entry = &doc["providers"]["p"];
        assert_eq!(
            entry["apis"][1]["api_key"],
            json!([{"file": "providers/p/api-key-embeddings"}])
        );
        assert_eq!(entry["apis"][1]["for"], json!(["kind:embedding"]));
        assert_eq!(entry["api_key"], json!({"file": "main.key"}));

        // The binding's effective config carries its own list, not the provider's.
        let eff = binding_config(entry, &entry["apis"][1]).unwrap();
        assert_eq!(lookup_in(home, &eff).key.as_deref(), Some("emb-key"));
        let chat = binding_config(entry, &entry["apis"][0]).unwrap();
        assert_eq!(lookup_in(home, &chat).key.as_deref(), Some("main-key"));
    }

    #[test]
    fn an_older_config_gets_its_key_file_listed_first() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        write_providers(
            home,
            "providers:\n  openai:\n    template: openai\n  nvidia:\n    template: nvidia\n  mine:\n    url: u\n    api_key: [{env: X}]\n",
        );
        std::fs::create_dir_all(home.join("providers/openai")).unwrap();
        std::fs::write(home.join("providers/openai/api-key"), "k\n").unwrap();
        std::fs::create_dir_all(home.join("providers/mine")).unwrap();
        std::fs::write(home.join("providers/mine/api-key"), "k\n").unwrap();
        upgrade_key_sources(home);
        assert_eq!(
            entry_key(home, "openai"),
            json!([{"file": "providers/openai/api-key"}, {"env": "OPENAI_API_KEY"}])
        );
        // No key file, or already the new form: left alone.
        assert!(entry_key(home, "nvidia").is_null());
        assert_eq!(entry_key(home, "mine"), json!([{"env": "X"}]));
        let before = std::fs::read_to_string(providers_config_path(home)).unwrap();
        upgrade_key_sources(home);
        assert_eq!(
            std::fs::read_to_string(providers_config_path(home)).unwrap(),
            before
        );
    }

    #[test]
    fn keychain_entries_become_key_files() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        write_providers(
            home,
            "providers:\n  openai:\n    template: openai\n    api_key: [{keychain: openai}, {env: OPENAI_API_KEY}]\n    apis:\n      - api: image-generations\n        api_key: {keychain: openai/image-generations}\n  both:\n    url: u\n    api_key: [{file: providers/both/api-key}, {keychain: both}]\n",
        );
        upgrade_key_sources(home);
        assert_eq!(
            entry_key(home, "openai"),
            json!([{"file": "providers/openai/api-key"}, {"env": "OPENAI_API_KEY"}])
        );
        let doc = read_providers_doc(home).unwrap();
        assert_eq!(
            doc["providers"]["openai"]["apis"][0]["api_key"],
            json!([{"file": "providers/openai/api-key-image-generations"}])
        );
        // Listed once, where the file already was.
        assert_eq!(
            entry_key(home, "both"),
            json!([{"file": "providers/both/api-key"}])
        );
        // The provider reads its key file again: saving one makes it work.
        let resolved = resolve_template(&doc["providers"]["openai"]).unwrap();
        assert!(key_sources(&resolved).is_ok());
        set_api_key_in(home, "openai", None, "sk-new").unwrap();
        assert_eq!(
            entry_key(home, "openai"),
            json!([{"file": "providers/openai/api-key"}, {"env": "OPENAI_API_KEY"}])
        );
    }

    #[test]
    fn deleting_an_alias_keeps_the_others_in_file_order() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        for (a, m) in [("one", "m1"), ("two", "m2"), ("three", "m3")] {
            set_model_alias_in(home, "p", a, m).unwrap();
        }
        delete_model_alias_in(home, "p", "one").unwrap();
        let names: Vec<String> = model_aliases_in(home, "p")
            .unwrap()
            .keys()
            .cloned()
            .collect();
        assert_eq!(names, ["two", "three"]);
    }
}
