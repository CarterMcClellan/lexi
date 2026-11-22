//! Where API keys come from. A provider's `api_key` in providers-config.yaml
//! is an ordered list of sources — `{env: VAR}`, `{keychain: NAME}`,
//! `{file: PATH}` (or the older `${VAR}` / literal strings) — and the first
//! one that has a key is used. Only a missing key (an unset variable, no
//! keychain item, no file) moves on to the next; a read error stops the lookup,
//! so a locked keychain never silently switches you to a different key.
//!
//! Keychain items live in the OS credential store (the login Keychain on
//! macOS, Credential Manager on Windows) under service `Lexi`, account NAME.
//! There's no OS store elsewhere, and `LEXI_KEY_STORE=file` (tests, CI,
//! headless machines) turns it off: keychain sources then count as missing and
//! a key Lexi saves goes in an owner-only file.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

/// The keychain service name every Lexi item is filed under.
const SERVICE: &str = "Lexi";

/// Whether this platform has an OS credential store Lexi can use.
pub(crate) const NATIVE: bool = cfg!(any(target_os = "macos", target_os = "windows"));

/// The kind of source Lexi adds when it saves a key and the config names
/// nowhere to put it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum KeyStore {
    Keychain,
    File,
}

impl KeyStore {
    /// The store in effect: `LEXI_KEY_STORE=file|keychain`, else the OS store
    /// when there is one.
    pub(crate) fn current() -> KeyStore {
        match std::env::var("LEXI_KEY_STORE").as_deref() {
            Ok("file") => KeyStore::File,
            _ if NATIVE => KeyStore::Keychain,
            _ => KeyStore::File,
        }
    }

    /// `"keychain"` or `"file"` — also the `source` a key read from it reports.
    pub(crate) fn id(self) -> &'static str {
        match self {
            KeyStore::Keychain => "keychain",
            KeyStore::File => "file",
        }
    }

    /// What to call it in messages.
    pub(crate) fn label(self) -> &'static str {
        match self {
            KeyStore::Keychain if cfg!(target_os = "windows") => "Windows Credential Manager",
            KeyStore::Keychain => "macOS Keychain",
            KeyStore::File => "an owner-only key file",
        }
    }
}

/// The keychain account Lexi names for `label` (a provider, or
/// `provider/api`) under `home`: a non-default `$LEXI_HOME` gets its own
/// accounts so separate homes never share keys.
pub(crate) fn account(home: &Path, label: &str) -> String {
    let default_home = dirs::home_dir().map(|h| h.join(".lexi-cli"));
    if default_home.as_deref() == Some(home) {
        label.to_string()
    } else {
        format!("{label} ({})", home.display())
    }
}

/// One place a key can come from: an entry of an `api_key` list. (No `Debug`:
/// a literal holds the key itself.)
#[derive(Clone, PartialEq, Eq)]
pub(crate) enum Source {
    /// An environment variable.
    Env(String),
    /// An OS keychain item (service `Lexi`, this account).
    Keychain(String),
    /// A file holding the key; relative paths are under `$LEXI_HOME`.
    File(String),
    /// The key written into providers-config.yaml itself.
    Literal(String),
}

const SOURCE_FORMS: &str =
    "api_key entries are {env: VAR}, {keychain: NAME}, {file: PATH} or \"${VAR}\"";

impl Source {
    /// `env`, `keychain`, `file` or `config` (a literal).
    pub(crate) fn kind(&self) -> &'static str {
        match self {
            Source::Env(_) => "env",
            Source::Keychain(_) => "keychain",
            Source::File(_) => "file",
            Source::Literal(_) => "config",
        }
    }

    /// Whether Lexi can save a key here (a keychain item or a file).
    pub(crate) fn writable(&self) -> bool {
        matches!(self, Source::Keychain(_) | Source::File(_))
    }

    /// Where it is, for messages: `$OPENAI_API_KEY`, `macOS Keychain item
    /// Lexi/openai`, a path.
    pub(crate) fn describe(&self) -> String {
        match self {
            Source::Env(v) => format!("${v}"),
            Source::Keychain(a) => {
                format!("{} item {SERVICE}/{a}", KeyStore::Keychain.label())
            }
            Source::File(p) => p.clone(),
            Source::Literal(_) => "providers-config.yaml (the key itself)".into(),
        }
    }

    /// As written in providers-config.yaml.
    pub(crate) fn to_value(&self) -> Value {
        match self {
            Source::Env(v) => json!({ "env": v }),
            Source::Keychain(a) => json!({ "keychain": a }),
            Source::File(p) => json!({ "file": p }),
            Source::Literal(k) => Value::from(k.as_str()),
        }
    }

    fn parse(v: &Value) -> Result<Option<Source>, String> {
        match v {
            Value::Null => Ok(None),
            Value::String(s) if s.trim().is_empty() => Ok(None),
            Value::String(s) => Ok(Some(
                match s.strip_prefix("${").and_then(|r| r.strip_suffix('}')) {
                    Some(var) => Source::Env(var.to_string()),
                    None => Source::Literal(s.clone()),
                },
            )),
            Value::Object(o) if o.len() == 1 => {
                let (k, v) = o.iter().next().expect("one entry");
                let name = v
                    .as_str()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| format!("api_key {k}: needs a name or path ({SOURCE_FORMS})"))?
                    .to_string();
                match k.as_str() {
                    "env" => Ok(Some(Source::Env(name))),
                    "keychain" => Ok(Some(Source::Keychain(name))),
                    "file" => Ok(Some(Source::File(name))),
                    other => Err(format!("unknown api_key source {other:?} ({SOURCE_FORMS})")),
                }
            }
            _ => Err(SOURCE_FORMS.to_string()),
        }
    }
}

/// An `api_key` value (a source, or a list of them) as its sources, in order.
pub(crate) fn parse_sources(v: Option<&Value>) -> Result<Vec<Source>, String> {
    match v {
        None => Ok(Vec::new()),
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|i| Source::parse(i).transpose())
            .collect(),
        Some(one) => Ok(Source::parse(one)?.into_iter().collect()),
    }
}

/// Sources as an `api_key` value (always a list).
pub(crate) fn sources_value(sources: &[Source]) -> Value {
    Value::Array(sources.iter().map(Source::to_value).collect())
}

/// A file source's path: `~/…` is under your home folder, a relative path is
/// under `$LEXI_HOME`.
pub(crate) fn file_path(home: &Path, raw: &str) -> PathBuf {
    if raw == "~" {
        return dirs::home_dir().unwrap_or_default();
    }
    if let Some(rest) = raw.strip_prefix("~/") {
        return dirs::home_dir().unwrap_or_default().join(rest);
    }
    let p = Path::new(raw);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        home.join(p)
    }
}

/// What looking at one source found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// It had the key that's used.
    Used,
    /// No key there (`not set`, `no item`, `no file`, `empty`) — the next
    /// source is tried.
    Missing(&'static str),
    /// It can't be used here (no keychain on this platform, or
    /// `LEXI_KEY_STORE=file`) — counts as missing.
    Unavailable(&'static str),
    /// Reading it failed; the lookup stops here.
    Failed(String),
    /// An earlier source decided the lookup.
    NotChecked,
}

impl Outcome {
    /// `used`, `missing`, `unavailable`, `error` or `not checked`.
    pub(crate) fn state(&self) -> &'static str {
        match self {
            Outcome::Used => "used",
            Outcome::Missing(_) => "missing",
            Outcome::Unavailable(_) => "unavailable",
            Outcome::Failed(_) => "error",
            Outcome::NotChecked => "not checked",
        }
    }

    pub(crate) fn note(&self) -> Option<String> {
        match self {
            Outcome::Missing(n) | Outcome::Unavailable(n) => Some((*n).to_string()),
            Outcome::Failed(e) => Some(e.clone()),
            _ => None,
        }
    }
}

/// The result of looking for a key through a list of sources.
pub(crate) struct Lookup {
    pub(crate) key: Option<String>,
    pub(crate) checked: Vec<(Source, Outcome)>,
}

impl Lookup {
    /// The source the key came from.
    pub(crate) fn used(&self) -> Option<&Source> {
        self.checked
            .iter()
            .find(|(_, o)| *o == Outcome::Used)
            .map(|(s, _)| s)
    }

    /// The source whose read failed, and why.
    pub(crate) fn failure(&self) -> Option<(&Source, &str)> {
        self.checked.iter().find_map(|(s, o)| match o {
            Outcome::Failed(e) => Some((s, e.as_str())),
            _ => None,
        })
    }

    /// `$OPENAI_API_KEY (not set), macOS Keychain item Lexi/openai (no item)`.
    pub(crate) fn summary(&self) -> String {
        self.checked
            .iter()
            .map(|(s, o)| match o.note() {
                Some(n) => format!("{} ({n})", s.describe()),
                None => s.describe(),
            })
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// Each source as `{kind, from, state, note?}`.
    pub(crate) fn to_value(&self) -> Value {
        Value::Array(
            self.checked
                .iter()
                .map(|(s, o)| {
                    let mut v =
                        json!({ "kind": s.kind(), "from": s.describe(), "state": o.state() });
                    if let Some(n) = o.note() {
                        v["note"] = Value::from(n);
                    }
                    v
                })
                .collect(),
        )
    }
}

/// Look through `sources` in order for a key.
pub(crate) fn lookup(home: &Path, sources: &[Source], secrets: Option<&dyn Secrets>) -> Lookup {
    let mut key = None;
    let mut checked = Vec::new();
    for source in sources {
        if key.is_some() || checked.iter().any(|(_, o)| matches!(o, Outcome::Failed(_))) {
            checked.push((source.clone(), Outcome::NotChecked));
            continue;
        }
        let outcome = match source {
            Source::Env(var) => match std::env::var(var) {
                Ok(v) if !v.trim().is_empty() => {
                    key = Some(v.trim().to_string());
                    Outcome::Used
                }
                _ => Outcome::Missing("not set"),
            },
            Source::Keychain(account) => match secrets {
                None if NATIVE => Outcome::Unavailable("LEXI_KEY_STORE=file"),
                None => Outcome::Unavailable("no keychain on this platform"),
                Some(store) => match store.get(account) {
                    Ok(Some(v)) if !v.trim().is_empty() => {
                        key = Some(v.trim().to_string());
                        Outcome::Used
                    }
                    Ok(_) => Outcome::Missing("no item"),
                    Err(e) => Outcome::Failed(e),
                },
            },
            Source::File(raw) => {
                let path = file_path(home, raw);
                if !path.exists() {
                    Outcome::Missing("no file")
                } else {
                    match std::fs::read_to_string(&path) {
                        Ok(v) if !v.trim().is_empty() => {
                            key = Some(v.trim().to_string());
                            Outcome::Used
                        }
                        Ok(_) => Outcome::Missing("empty"),
                        Err(e) => Outcome::Failed(e.to_string()),
                    }
                }
            }
            Source::Literal(k) => {
                key = Some(k.trim().to_string());
                Outcome::Used
            }
        };
        checked.push((source.clone(), outcome));
    }
    Lookup { key, checked }
}

/// A secret store keyed by account (the OS keychain, or a map in tests).
pub(crate) trait Secrets {
    /// The saved secret, `None` when there's no item.
    fn get(&self, account: &str) -> Result<Option<String>, String>;
    fn set(&self, account: &str, secret: &str) -> Result<(), String>;
    /// Delete the item; whether there was one.
    fn delete(&self, account: &str) -> Result<bool, String>;
}

/// The OS credential store.
pub(crate) struct OsSecrets;

#[cfg(any(target_os = "macos", target_os = "windows"))]
impl Secrets for OsSecrets {
    fn get(&self, account: &str) -> Result<Option<String>, String> {
        let entry = keyring::Entry::new(SERVICE, account).map_err(|e| e.to_string())?;
        match entry.get_password() {
            Ok(s) => Ok(Some(s)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }
    fn set(&self, account: &str, secret: &str) -> Result<(), String> {
        keyring::Entry::new(SERVICE, account)
            .and_then(|e| e.set_password(secret))
            .map_err(|e| e.to_string())
    }
    fn delete(&self, account: &str) -> Result<bool, String> {
        let entry = keyring::Entry::new(SERVICE, account).map_err(|e| e.to_string())?;
        match entry.delete_credential() {
            Ok(()) => Ok(true),
            Err(keyring::Error::NoEntry) => Ok(false),
            Err(e) => Err(e.to_string()),
        }
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
impl Secrets for OsSecrets {
    fn get(&self, _: &str) -> Result<Option<String>, String> {
        Ok(None)
    }
    fn set(&self, _: &str, _: &str) -> Result<(), String> {
        Err("this platform has no supported credential store".into())
    }
    fn delete(&self, _: &str) -> Result<bool, String> {
        Ok(false)
    }
}

/// The OS keychain, or `None` where there isn't one (or `LEXI_KEY_STORE=file`).
pub(crate) fn current_secrets() -> Option<&'static dyn Secrets> {
    match KeyStore::current() {
        KeyStore::Keychain => Some(&OsSecrets),
        KeyStore::File => None,
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::cell::RefCell;
    use std::collections::HashMap;

    /// An in-memory secret store; `fail` makes every call error.
    #[derive(Default)]
    pub(crate) struct MemSecrets {
        pub(crate) items: RefCell<HashMap<String, String>>,
        pub(crate) fail: bool,
    }

    impl super::Secrets for MemSecrets {
        fn get(&self, account: &str) -> Result<Option<String>, String> {
            if self.fail {
                return Err("keychain locked".into());
            }
            Ok(self.items.borrow().get(account).cloned())
        }
        fn set(&self, account: &str, secret: &str) -> Result<(), String> {
            if self.fail {
                return Err("keychain locked".into());
            }
            self.items
                .borrow_mut()
                .insert(account.to_string(), secret.to_string());
            Ok(())
        }
        fn delete(&self, account: &str) -> Result<bool, String> {
            if self.fail {
                return Err("keychain locked".into());
            }
            Ok(self.items.borrow_mut().remove(account).is_some())
        }
    }
}
