//! Where API keys come from. A provider's `api_key` in providers-config.yaml
//! is an ordered list of sources — `{env: VAR}`, `{file: PATH}` (or the older
//! `${VAR}` / literal strings) — and the first one that has a key is used.
//! Only a missing key (an unset variable, no file, an empty file) moves on to
//! the next; a read error stops the lookup, so an unreadable file never
//! silently switches you to a different key. Keys Lexi saves go in
//! owner-only files (mode 0600) under `$LEXI_HOME`.
//!
//! A development build briefly kept keys in the OS keychain
//! (`{keychain: NAME}`); `ensure_home` rewrites such entries to the
//! provider's key file, and one written by hand is a config error that says
//! what to use instead.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

/// One place a key can come from: an entry of an `api_key` list. (No `Debug`:
/// a literal holds the key itself.)
#[derive(Clone, PartialEq, Eq)]
pub(crate) enum Source {
    /// An environment variable.
    Env(String),
    /// A file holding the key; relative paths are under `$LEXI_HOME`.
    File(String),
    /// The key written into providers-config.yaml itself.
    Literal(String),
}

const SOURCE_FORMS: &str = "api_key entries are {env: VAR}, {file: PATH} or \"${VAR}\"";

impl Source {
    /// `env`, `file` or `config` (a literal).
    pub(crate) fn kind(&self) -> &'static str {
        match self {
            Source::Env(_) => "env",
            Source::File(_) => "file",
            Source::Literal(_) => "config",
        }
    }

    /// Whether Lexi can save a key here (a file).
    pub(crate) fn writable(&self) -> bool {
        matches!(self, Source::File(_))
    }

    /// Where it is, for messages: `$OPENAI_API_KEY`, a path.
    pub(crate) fn describe(&self) -> String {
        match self {
            Source::Env(v) => format!("${v}"),
            Source::File(p) => p.clone(),
            Source::Literal(_) => "providers-config.yaml (the key itself)".into(),
        }
    }

    /// As written in providers-config.yaml.
    pub(crate) fn to_value(&self) -> Value {
        match self {
            Source::Env(v) => json!({ "env": v }),
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
                    "file" => Ok(Some(Source::File(name))),
                    "keychain" => Err(format!(
                        "api_key {{keychain: {name}}}: Lexi doesn't use the system keychain; save the key again (the desktop app's Setup Wizard, or `lexi providers api-key set`) or list it as {{file: PATH}} or {{env: VAR}}"
                    )),
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
    /// No key there (`not set`, `no file`, `empty`) — the next source is tried.
    Missing(&'static str),
    /// Reading it failed; the lookup stops here.
    Failed(String),
    /// An earlier source decided the lookup.
    NotChecked,
}

impl Outcome {
    /// `used`, `missing`, `error` or `not checked`.
    pub(crate) fn state(&self) -> &'static str {
        match self {
            Outcome::Used => "used",
            Outcome::Missing(_) => "missing",
            Outcome::Failed(_) => "error",
            Outcome::NotChecked => "not checked",
        }
    }

    pub(crate) fn note(&self) -> Option<String> {
        match self {
            Outcome::Missing(n) => Some((*n).to_string()),
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

    /// `$OPENAI_API_KEY (not set), providers/openai/api-key (no file)`.
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
pub(crate) fn lookup(home: &Path, sources: &[Source]) -> Lookup {
    let mut key = None;
    let mut checked: Vec<(Source, Outcome)> = Vec::new();
    for source in sources {
        let decided = checked
            .iter()
            .any(|(_, o)| matches!(o, Outcome::Used | Outcome::Failed(_)));
        if decided {
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
