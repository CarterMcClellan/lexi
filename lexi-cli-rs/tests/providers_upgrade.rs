//! M5: `providers upgrade` (older entries → templates, keeping your changes)
//! and `providers api list|add|rm` (a provider's bindings).

mod common;

use common::{lexi_out, seeded_home, stderr, stdout};
use serde_json::{json, Value};

/// A home whose providers-config.yaml is the pre-template shipped file, with
/// one customized provider.
fn legacy_home() -> tempfile::TempDir {
    let home = seeded_home();
    let legacy = include_str!("../../lexi-core/providers/legacy-providers-config.yaml").replace(
        "    default_model: meta/llama-3.1-70b-instruct",
        "    default_model: nvidia/nemotron-3-ultra-550b-a55b\n    timeout: 30",
    );
    std::fs::write(home.path().join("providers/providers-config.yaml"), legacy).unwrap();
    home
}

fn json(home: &std::path::Path, args: &[&str]) -> Value {
    let mut a = vec!["--json"];
    a.extend_from_slice(args);
    let o = lexi_out(home, &a);
    assert!(o.status.success(), "{args:?}: {}", stderr(&o));
    serde_json::from_slice(&o.stdout).unwrap()
}

fn config_yaml(home: &std::path::Path) -> Value {
    let text = std::fs::read_to_string(home.join("providers/providers-config.yaml")).unwrap();
    serde_yaml::from_str(&text).unwrap()
}

#[test]
fn upgrade_keeps_only_your_changes() {
    let home = legacy_home();
    let h = home.path();

    // Dry run: shows the change, writes nothing.
    let before = std::fs::read_to_string(h.join("providers/providers-config.yaml")).unwrap();
    let plan = json(h, &["providers", "upgrade"]);
    assert_eq!(plan.as_array().unwrap().len(), 3);
    assert_eq!(
        plan[0]["after"],
        json!({"template": "openai"}),
        "all defaults: dropped"
    );
    assert_eq!(
        plan[2]["after"],
        json!({"template": "nvidia", "timeout": 30,
               "apis": [{"api": "chat-completions", "default_model": "nvidia/nemotron-3-ultra-550b-a55b"}]}),
        "custom default model → binding; unknown keys kept"
    );
    assert_eq!(
        std::fs::read_to_string(h.join("providers/providers-config.yaml")).unwrap(),
        before
    );
    assert!(stdout(&lexi_out(h, &["providers", "upgrade"]))
        .ends_with("Run again with --yes to apply.\n"));

    // Apply: backed up, rewritten, and the templates now apply.
    let done = json(h, &["providers", "upgrade", "--yes"]);
    let backup = done[0]["backup"].as_str().unwrap();
    assert_eq!(std::fs::read_to_string(backup).unwrap(), before);
    let cfg = config_yaml(h);
    assert_eq!(cfg["providers"]["openai"], json!({"template": "openai"}));
    assert_eq!(
        cfg["defaults"]["max_tokens"], 4096,
        "the rest of the file is kept"
    );
    let cls = json(h, &["models", "tags", "openai", "gpt-image-2"]);
    assert_eq!(cls["api"], "image-generations");

    // Idempotent.
    let again = json(h, &["providers", "upgrade", "openai"]);
    assert_eq!(again[0]["changed"], false);

    let o = lexi_out(h, &["providers", "upgrade", "openai", "--template", "nope"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(
        stderr(&o).contains("unknown provider template \"nope\""),
        "{}",
        stderr(&o)
    );
}

#[test]
fn bindings_list_add_and_remove() {
    let home = seeded_home();
    let h = home.path();

    let apis = json(h, &["providers", "api", "list", "openai"]);
    let names: Vec<&str> = apis
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["api"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["responses", "image-generations", "embeddings"]);
    assert!(apis
        .as_array()
        .unwrap()
        .iter()
        .all(|b| b["source"] == "template" && b["enabled"] == true));

    // Removing a template API turns it off; image models then have no API.
    json(
        h,
        &["providers", "api", "rm", "openai", "image-generations"],
    );
    let off = &json(h, &["providers", "api", "list", "openai"])[1];
    assert_eq!(
        (off["enabled"].clone(), off["source"].clone()),
        (json!(false), json!("user"))
    );
    assert!(json(h, &["models", "tags", "openai", "gpt-image-2"])["api"].is_null());
    // Adding it back re-enables it and leaves no leftover entry.
    let v = json(
        h,
        &["providers", "api", "add", "openai", "image-generations"],
    );
    assert_eq!(v["existed"], true);
    assert_eq!(
        config_yaml(h)["providers"]["openai"],
        json!({"template": "openai"})
    );

    // A new binding on an older single-API provider converts it to a list.
    std::fs::write(
        h.join("providers/providers-config.yaml"),
        "providers:\n  local:\n    url: http://localhost:8000/v1\n    api: chat-completions\n    api_key: k\n    default_model: llama\n",
    )
    .unwrap();
    let v = json(
        h,
        &[
            "providers",
            "api",
            "add",
            "local",
            "embeddings",
            "--for",
            "kind:embedding",
            "--default-model",
            "bge",
        ],
    );
    assert_eq!(v["existed"], false);
    assert_eq!(
        config_yaml(h)["providers"]["local"]["apis"],
        json!([{"api": "chat-completions", "default_model": "llama"},
               {"api": "embeddings", "for": ["kind:embedding"], "default_model": "bge"}])
    );
    json(h, &["models", "tag", "local", "bge", "+kind:embedding"]);
    assert_eq!(
        json(h, &["models", "tags", "local", "bge"])["api"],
        "embeddings"
    );

    // Guard rails.
    let o = lexi_out(h, &["providers", "api", "add", "local", "nope"]);
    assert!(
        stderr(&o).contains("unknown API definition"),
        "{}",
        stderr(&o)
    );
    let o = lexi_out(
        h,
        &[
            "providers",
            "api",
            "add",
            "local",
            "embeddings",
            "--for",
            "kind:nope",
        ],
    );
    assert!(stderr(&o).contains("unknown tag"), "{}", stderr(&o));
    json(h, &["providers", "api", "rm", "local", "embeddings"]);
    let o = lexi_out(h, &["providers", "api", "rm", "local", "chat-completions"]);
    assert!(stderr(&o).contains("only API"), "{}", stderr(&o));
}
