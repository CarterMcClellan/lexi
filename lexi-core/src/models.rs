//! Model classification: the tag vocabulary, tag rules, user tags, and choosing
//! which of a provider's API bindings serves a model. Pure (no I/O); pinned by
//! `conformance/cases/model/`. Design: `docs/provider-model-design.md`.
//!
//! A model's tags come from layers, applied in order (later wins):
//! 1. the chosen binding's API definition (`capabilities`),
//! 2. the provider's `tag_rules` (model-id glob → tag ops),
//! 3. catalog data (tag ops derived from the provider's model listing),
//! 4. the user's tags (`model-tags.yaml`: id or glob → tag ops).
//!
//! Layers 2–4 also decide the binding: a model goes to the first binding whose
//! `for:` tags it has, else to a catch-all binding (no `for:`) whose API serves
//! the model's `kind:`.

use serde_json::{json, Map, Value};

/// The closed tag vocabulary: namespace → allowed names. `label:<anything>` is
/// also accepted (free-form, for filtering only).
pub const TAG_VOCABULARY: &[(&str, &[&str])] = &[
    ("in", &["text", "image", "audio", "video", "document"]),
    ("out", &["text", "image", "audio", "video", "vector"]),
    (
        "kind",
        &[
            "chat",
            "embedding",
            "image-generation",
            "speech",
            "transcription",
            "rerank",
            "classifier",
            "detector",
        ],
    ),
    (
        "feature",
        &[
            "tools",
            "thinking",
            "structured-output",
            "web-search",
            "streaming",
            "background",
            "files",
        ],
    ),
    ("status", &["preview", "deprecated"]),
];

/// The `in:` / `out:` tags a `kind:` implies.
fn kind_io(kind: &str) -> (&'static [&'static str], &'static [&'static str]) {
    match kind {
        "chat" => (&["text"], &["text"]),
        "embedding" => (&["text"], &["vector"]),
        "image-generation" => (&["text"], &["image"]),
        "speech" => (&["text"], &["audio"]),
        "transcription" => (&["audio"], &["text"]),
        "rerank" | "classifier" => (&["text"], &[]),
        "detector" => (&["video"], &[]),
        _ => (&[], &[]),
    }
}

/// The vocabulary as data: `{namespaces: {in: [...], ...}, kinds: {chat:
/// {in, out}, ...}, labels: "label:<name>"}`.
pub fn tag_vocabulary() -> Value {
    let mut ns = Map::new();
    for (name, tags) in TAG_VOCABULARY {
        ns.insert(name.to_string(), json!(tags));
    }
    let mut kinds = Map::new();
    for kind in TAG_VOCABULARY[2].1 {
        let (i, o) = kind_io(kind);
        kinds.insert(kind.to_string(), json!({ "in": i, "out": o }));
    }
    json!({ "namespaces": ns, "kinds": kinds, "labels": "label:<name>" })
}

/// Check one tag against the vocabulary.
pub fn validate_tag(tag: &str) -> Result<(), String> {
    let Some((ns, name)) = tag.split_once(':') else {
        return Err(format!(
            "invalid tag '{tag}': expected namespace:name (e.g. out:image)"
        ));
    };
    if ns == "label" {
        if name.is_empty() || name.chars().any(char::is_whitespace) {
            return Err(format!(
                "invalid label '{tag}': use label:<name> without spaces"
            ));
        }
        return Ok(());
    }
    match TAG_VOCABULARY.iter().find(|(n, _)| *n == ns) {
        None => Err(format!(
            "unknown tag namespace '{ns}' in '{tag}' (namespaces: in, out, kind, feature, status, label)"
        )),
        Some((_, names)) if !names.contains(&name) => Err(format!(
            "unknown tag '{tag}' (allowed {ns}: tags: {})",
            names.join(", ")
        )),
        Some(_) => Ok(()),
    }
}

/// Parse a tag operation: `tag` or `+tag` adds, `-tag` removes.
pub fn parse_tag_op(op: &str) -> Result<(bool, String), String> {
    let (add, tag) = match op.strip_prefix('-') {
        Some(t) => (false, t),
        None => (true, op.strip_prefix('+').unwrap_or(op)),
    };
    validate_tag(tag)?;
    Ok((add, tag.to_string()))
}

/// Glob match: `*` matches any run of characters (including `/`), `?` one.
pub fn glob_match(pattern: &str, text: &str) -> bool {
    let (p, t): (Vec<char>, Vec<char>) = (pattern.chars().collect(), text.chars().collect());
    let (mut pi, mut ti) = (0, 0);
    let (mut star, mut mark) = (None, 0);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

/// The `kind:` an API definition serves: `capabilities.kind`, else derived from
/// its outputs (text → chat, image → image-generation, vector → embedding,
/// audio → speech).
pub fn api_kind(api_def: &Value) -> String {
    let cap = api_def.get("capabilities");
    if let Some(k) = cap.and_then(|c| c.get("kind")).and_then(Value::as_str) {
        return k.to_string();
    }
    let outs: Vec<&str> = cap
        .and_then(|c| c.get("output"))
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let kind = if outs.contains(&"text") {
        "chat"
    } else if outs.contains(&"image") {
        "image-generation"
    } else if outs.contains(&"vector") {
        "embedding"
    } else if outs.contains(&"audio") {
        "speech"
    } else {
        "chat"
    };
    kind.to_string()
}

/// The tags an API definition gives every model it serves.
pub fn api_tags(api_def: &Value) -> Vec<String> {
    let cap = api_def.get("capabilities");
    let list = |key: &str| -> Vec<String> {
        cap.and_then(|c| c.get(key))
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default()
    };
    let mut tags = vec![format!("kind:{}", api_kind(api_def))];
    tags.extend(list("input").into_iter().map(|m| format!("in:{m}")));
    tags.extend(list("output").into_iter().map(|m| format!("out:{m}")));
    tags.extend(list("features").into_iter().map(|f| format!("feature:{f}")));
    tags
}

/// A provider's bindings, normalized to `[{api, for?, default_model?, …}]`.
/// Reads `apis:` (entries may be bare API names) or the older single `api:`
/// (+ `default_model`). Bindings marked `enabled: false` (how a user turns off
/// a template's binding) are left out.
pub fn provider_bindings(provider: &Value) -> Vec<Value> {
    all_bindings(provider)
        .into_iter()
        .filter(|b| b.get("enabled") != Some(&Value::Bool(false)))
        .collect()
}

/// [`provider_bindings`] including disabled ones.
fn all_bindings(provider: &Value) -> Vec<Value> {
    if let Some(list) = provider.get("apis").and_then(Value::as_array) {
        return list
            .iter()
            .filter_map(|b| match b {
                Value::String(name) => Some(json!({ "api": name })),
                Value::Object(m) if m.get("api").is_some_and(Value::is_string) => Some(b.clone()),
                _ => None,
            })
            .collect();
    }
    match provider.get("api").and_then(Value::as_str) {
        Some(api) => {
            let mut b = json!({ "api": api });
            if let Some(m) = provider.get("default_model") {
                b["default_model"] = m.clone();
            }
            vec![b]
        }
        None => Vec::new(),
    }
}

/// An ordered tag set that remembers each tag's source.
#[derive(Default)]
struct TagSet(Vec<(String, String)>);

impl TagSet {
    fn has(&self, tag: &str) -> bool {
        self.0.iter().any(|(t, _)| t == tag)
    }
    fn kind(&self) -> Option<&str> {
        self.0.iter().find_map(|(t, _)| t.strip_prefix("kind:"))
    }
    fn add(&mut self, tag: &str, source: &str) {
        if tag.starts_with("kind:") {
            self.0.retain(|(t, _)| !t.starts_with("kind:"));
        }
        match self.0.iter_mut().find(|(t, _)| t == tag) {
            Some(entry) => entry.1 = source.to_string(),
            None => self.0.push((tag.to_string(), source.to_string())),
        }
    }
    fn remove(&mut self, tag: &str) {
        self.0.retain(|(t, _)| t != tag);
    }
    /// Apply `ops` (`+tag` / `tag` / `-tag`); invalid ops are skipped (they are
    /// rejected when written). With `first_kind_wins`, a `kind:` already set in
    /// this layer is kept.
    fn apply(&mut self, ops: &Value, source: &str, kind_set: &mut bool, first_kind_wins: bool) {
        for op in ops
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            let Ok((add, tag)) = parse_tag_op(op) else {
                continue;
            };
            if tag.starts_with("kind:") && add {
                if first_kind_wins && *kind_set {
                    continue;
                }
                *kind_set = true;
            }
            if add {
                self.add(&tag, source);
            } else {
                self.remove(&tag);
            }
        }
    }
    fn sorted(&self) -> Vec<Value> {
        let rank = |t: &str| {
            let ns = t.split(':').next().unwrap_or("");
            TAG_VOCABULARY
                .iter()
                .position(|(n, _)| *n == ns)
                .unwrap_or(TAG_VOCABULARY.len())
        };
        let mut v: Vec<&(String, String)> = self.0.iter().collect();
        v.sort_by(|a, b| (rank(&a.0), &a.0).cmp(&(rank(&b.0), &b.0)));
        v.into_iter()
            .map(|(t, s)| json!({ "tag": t, "source": s }))
            .collect()
    }
}

/// Apply the classification layers (rules, catalog, user) to `set`.
fn apply_layers(set: &mut TagSet, provider: &Value, model: &str, catalog: &Value, user: &Value) {
    let mut kind_set = false;
    if let Some(rules) = provider.get("tag_rules").and_then(Value::as_object) {
        for (pattern, ops) in rules {
            if glob_match(pattern, model) {
                set.apply(ops, &format!("rule:{pattern}"), &mut kind_set, true);
            }
        }
    }
    let mut k = false;
    set.apply(catalog, "catalog", &mut k, false);
    if let Some(user) = user.as_object() {
        for (key, ops) in user {
            if glob_match(key, model) {
                let mut k = false;
                set.apply(ops, "user", &mut k, false);
            }
        }
    }
}

/// Classify `model` for `provider` (its resolved entry: `apis`, `tag_rules`, …).
///
/// - `api_defs`: the bindings' API definitions by name;
/// - `user_tags`: the provider's `model-tags.yaml` (`{id-or-glob: [ops]}`);
/// - `catalog_tags`: tag ops derived from the provider's listing (`[ops]`);
/// - `pinned_api`: a configured model's explicit `api:` (wins outright).
///
/// Returns `{model, kind, api, binding, tags: [{tag, source}]}` — `binding` is
/// the index into the provider's bindings (null when pinned to an API it doesn't
/// list) — or `{model, kind, api: null, binding: null, tags, error}` when no
/// binding serves the model.
pub fn classify_model(
    provider: &Value,
    api_defs: &Map<String, Value>,
    model: &str,
    user_tags: &Value,
    catalog_tags: &Value,
    pinned_api: Option<&str>,
) -> Value {
    let bindings = provider_bindings(provider);

    // Phase 1: what the model is, from rules / catalog / user (no API yet).
    let mut probe = TagSet::default();
    apply_layers(&mut probe, provider, model, catalog_tags, user_tags);
    let kind = probe.kind().map(str::to_string);
    if let Some(k) = &kind {
        let (i, o) = kind_io(k);
        for m in i {
            probe.add(&format!("in:{m}"), "kind");
        }
        for m in o {
            probe.add(&format!("out:{m}"), "kind");
        }
    }

    let def_of = |b: &Value| {
        b.get("api")
            .and_then(Value::as_str)
            .and_then(|n| api_defs.get(n))
    };
    let chosen: Option<(Option<usize>, String)> = if let Some(api) = pinned_api {
        let idx = bindings
            .iter()
            .position(|b| b.get("api").and_then(Value::as_str) == Some(api));
        Some((idx, api.to_string()))
    } else {
        let by_tags = bindings.iter().position(|b| {
            b.get("for").and_then(Value::as_array).is_some_and(|req| {
                !req.is_empty() && req.iter().filter_map(Value::as_str).all(|t| probe.has(t))
            })
        });
        let catch_all = || {
            bindings.iter().position(|b| {
                b.get("for").is_none()
                    && def_of(b).is_some_and(|d| kind.as_deref().is_none_or(|k| api_kind(d) == k))
            })
        };
        by_tags.or_else(catch_all).map(|i| {
            (
                Some(i),
                bindings[i]["api"].as_str().unwrap_or("").to_string(),
            )
        })
    };

    let Some((binding, api)) = chosen else {
        let what = match &kind {
            Some(k) => format!(
                "{model} is a kind:{k} model, and this provider has no API configured for it"
            ),
            None => format!("no API configured for model {model}"),
        };
        return json!({
            "model": model, "kind": kind, "api": null, "binding": null,
            "tags": probe.sorted(), "error": what,
        });
    };

    // Phase 2: the API's tags, then the same layers on top.
    let mut tags = TagSet::default();
    if let Some(def) = api_defs.get(&api) {
        for t in api_tags(def) {
            tags.add(&t, &format!("api:{api}"));
        }
    }
    apply_layers(&mut tags, provider, model, catalog_tags, user_tags);
    json!({
        "model": model,
        "kind": tags.kind(),
        "api": api,
        "binding": binding,
        "tags": tags.sorted(),
    })
}

/// Evaluate one catalog value rule against a listing entry:
/// `Some(true)` = the fact holds, `Some(false)` = it doesn't, `None` = unknown
/// (the field is missing). Rules: `{path}` (a boolean field), `{path, equals}`,
/// `{path, in: [...]}`, `{path, present: true}` (non-null = holds, else
/// unknown — never "doesn't"), `{const: bool}`.
fn eval_rule(entry: &Value, rule: &Value) -> Option<bool> {
    if let Some(c) = rule.get("const") {
        return c.as_bool();
    }
    let path = rule.get("path").and_then(Value::as_str)?;
    let v = crate::get_path(entry, path).filter(|v| !v.is_null());
    if rule.get("present").and_then(Value::as_bool) == Some(true) {
        return v.map(|_| true);
    }
    let v = v?;
    if let Some(want) = rule.get("equals") {
        return Some(v == want);
    }
    if let Some(list) = rule.get("in").and_then(Value::as_array) {
        return Some(list.contains(v));
    }
    v.as_bool()
}

/// Tag ops derived from a provider's model-listing entry by the catalog's
/// `tags` mapping (`{tag: rule}`): a rule that holds adds the tag, one that
/// doesn't removes it, an unknown one does nothing.
pub fn catalog_tags(entry: &Value, catalog: &Value) -> Vec<String> {
    let Some(rules) = catalog.get("tags").and_then(Value::as_object) else {
        return Vec::new();
    };
    rules
        .iter()
        .filter(|(tag, _)| validate_tag(tag).is_ok())
        .filter_map(|(tag, rule)| match eval_rule(entry, rule) {
            Some(true) => Some(format!("+{tag}")),
            Some(false) => Some(format!("-{tag}")),
            None => None,
        })
        .collect()
}

/// Limits and a display name from a listing entry, by the catalog's `limits`
/// (`{name: {path}}`) and `name` (`{path}`) mappings: `{name?, limits: {…}}`.
pub fn catalog_facts(entry: &Value, catalog: &Value) -> Value {
    let read = |rule: &Value| {
        rule.get("path")
            .and_then(Value::as_str)
            .and_then(|p| crate::get_path(entry, p))
            .filter(|v| !v.is_null())
            .cloned()
    };
    let mut limits = Map::new();
    if let Some(m) = catalog.get("limits").and_then(Value::as_object) {
        for (k, rule) in m {
            if let Some(v) = read(rule) {
                limits.insert(k.clone(), v);
            }
        }
    }
    let mut out = json!({ "limits": limits });
    if let Some(name) = catalog.get("name").and_then(read) {
        out["name"] = name;
    }
    out
}

/// The first binding whose API produces `output` (`text`, `image`, …).
pub fn binding_for_output(
    provider: &Value,
    api_defs: &Map<String, Value>,
    output: &str,
) -> Option<usize> {
    provider_bindings(provider).iter().position(|b| {
        b.get("api")
            .and_then(Value::as_str)
            .and_then(|n| api_defs.get(n))
            .and_then(|d| d.pointer("/capabilities/output"))
            .and_then(Value::as_array)
            .is_some_and(|outs| outs.iter().any(|o| o.as_str() == Some(output)))
    })
}

fn merge_values(base: &Value, overlay: &Value) -> Value {
    match (base, overlay) {
        (Value::Object(b), Value::Object(o)) => {
            let mut out = b.clone();
            for (k, v) in o {
                let merged = match out.get(k) {
                    Some(existing) => merge_values(existing, v),
                    None => v.clone(),
                };
                out.insert(k.clone(), merged);
            }
            Value::Object(out)
        }
        _ => overlay.clone(),
    }
}

/// A provider entry over its shipped template: a deep merge, except that
/// `apis` merge by `api` name (the user's fields win; new bindings are added
/// first) and `tag_rules` merge by pattern (the user's rules first). Keys named
/// in the entry's `replace: [...]` replace the template's outright.
pub fn merge_provider_template(template: &Value, entry: &Value) -> Value {
    let replace: Vec<&str> = entry
        .get("replace")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let mut overlay = entry.clone();
    let mut special = Map::new();
    if let Some(o) = overlay.as_object_mut() {
        o.shift_remove("template");
        o.shift_remove("replace");
        for key in ["apis", "tag_rules"] {
            if let Some(v) = o.shift_remove(key) {
                special.insert(key.to_string(), v);
            }
        }
    }
    let mut out = merge_values(template, &overlay);

    if let Some(user) = special.get("apis") {
        let merged = if replace.contains(&"apis") {
            user.clone()
        } else {
            let base = all_bindings(template);
            let user_b = all_bindings(&json!({ "apis": user }));
            let mut result: Vec<Value> = user_b
                .iter()
                .filter(|u| !base.iter().any(|b| b["api"] == u["api"]))
                .cloned()
                .collect();
            for b in base {
                match user_b.iter().find(|u| u["api"] == b["api"]) {
                    Some(u) => result.push(merge_values(&b, u)),
                    None => result.push(b),
                }
            }
            Value::Array(result)
        };
        out["apis"] = merged;
    }
    if let Some(user) = special.get("tag_rules") {
        let merged = if replace.contains(&"tag_rules") {
            user.clone()
        } else {
            let mut m = user.as_object().cloned().unwrap_or_default();
            if let Some(base) = template.get("tag_rules").and_then(Value::as_object) {
                for (k, v) in base {
                    m.entry(k.clone()).or_insert_with(|| v.clone());
                }
            }
            Value::Object(m)
        };
        out["tag_rules"] = merged;
    }
    if let Some(o) = out.as_object_mut() {
        o.shift_remove("template");
        // A templated entry defines `apis`; drop an inherited single `api`.
        if o.contains_key("apis") {
            o.shift_remove("api");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globs() {
        assert!(glob_match("gpt-image-*", "gpt-image-2.5-flare"));
        assert!(glob_match("*embed*", "nvidia/nv-embedqa-e5-v5"));
        assert!(glob_match("meta/*", "meta/llama-3"));
        assert!(glob_match("a?c", "abc"));
        assert!(!glob_match("gpt-*", "chatgpt-4"));
        assert!(glob_match("exact", "exact"));
        assert!(!glob_match("exact", "exactly"));
    }

    #[test]
    fn vocabulary_checks() {
        assert!(validate_tag("out:image").is_ok());
        assert!(validate_tag("label:fast").is_ok());
        assert!(validate_tag("out:smell")
            .unwrap_err()
            .contains("allowed out:"));
        assert!(validate_tag("color:red").unwrap_err().contains("namespace"));
        assert!(validate_tag("image").is_err());
        assert_eq!(
            parse_tag_op("-in:image").unwrap(),
            (false, "in:image".into())
        );
        assert_eq!(
            parse_tag_op("+kind:chat").unwrap(),
            (true, "kind:chat".into())
        );
    }

    #[test]
    fn older_single_api_reads_as_one_binding() {
        let b = provider_bindings(&json!({"api": "messages", "default_model": "m"}));
        assert_eq!(b, vec![json!({"api": "messages", "default_model": "m"})]);
    }
}
