//! Run the shared language-neutral conformance suite against `lexi-core`.
//!
//! These are the *same* fixtures the Python runner uses
//! (`lexi-cli/tests/test_conformance.py`), so passing here proves behavioral
//! parity between the two engines. API definitions are resolved by name from
//! this crate's own bundled copy under `apis/`.

use std::fs;
use std::path::{Path, PathBuf};

use lexi_core as core;
use serde_json::{Map, Value};

fn conformance_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../conformance")
}

fn apis_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("apis")
}

fn load_json(path: &Path) -> Value {
    let text = fs::read_to_string(path).unwrap_or_else(|e| panic!("read {path:?}: {e}"));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("parse {path:?}: {e}"))
}

fn load_api(name: &str) -> Value {
    let path = apis_dir().join(format!("{name}.yaml"));
    let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"));
    serde_yaml::from_str(&text).unwrap_or_else(|e| panic!("parse {path:?}: {e}"))
}

fn case_files(category: &str) -> Vec<PathBuf> {
    let dir = conformance_dir().join("cases").join(category);
    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read_dir {dir:?}: {e}"))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().map(|x| x == "json").unwrap_or(false))
        .collect();
    files.sort();
    files
}

fn name_of(path: &Path) -> String {
    path.file_stem().unwrap().to_string_lossy().into_owned()
}

fn finish(category: &str, failures: Vec<String>) {
    assert!(
        failures.is_empty(),
        "\n{} conformance failures in `{category}`:\n  {}",
        failures.len(),
        failures.join("\n  ")
    );
}

#[test]
fn request_composition() {
    let mut failures = Vec::new();
    let files = case_files("request");
    assert!(!files.is_empty(), "no request cases found");

    for path in files {
        let case = load_json(&path);
        let api = load_api(case["api"].as_str().unwrap());
        let compose = &case["compose"];

        // Mirror respond_with_provider: normalize the message (or a canonical
        // `message` with image parts) and lower its content via content_mapping.
        let payload = compose
            .get("prompt_payload")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        let msg = match compose.get("message") {
            Some(m) => core::message_from_dict(m),
            None => core::normalize_prompt_payload(&Value::Object(payload.clone())),
        };
        let bindings = core::lower_message(&msg, api.get("content_mapping"));
        let mut effective = payload.clone();
        for (k, v) in bindings.as_object().unwrap() {
            effective.insert(k.clone(), v.clone());
        }
        let pp = Value::Object(effective);
        let ov = compose.get("overrides").cloned().unwrap_or(Value::Null);
        let presets: Vec<String> = compose
            .get("tool_presets")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let set_paths: Vec<(String, Value)> = compose
            .get("set_paths")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .map(|pair| (pair[0].as_str().unwrap().to_string(), pair[1].clone()))
                    .collect()
            })
            .unwrap_or_default();
        let unset_paths: Vec<String> = compose
            .get("unset_paths")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        match core::compose_request_body(&api, &pp, &ov, &presets, &set_paths, &unset_paths, false)
        {
            Ok(body) => {
                if body != case["expect_body"] {
                    failures.push(format!(
                        "{}: body mismatch\n    got:      {}\n    expected: {}",
                        name_of(&path),
                        body,
                        case["expect_body"]
                    ));
                }
            }
            Err(e) => failures.push(format!("{}: compose error: {e}", name_of(&path))),
        }
    }
    finish("request", failures);
}

#[test]
fn response_extraction() {
    let mut failures = Vec::new();
    let files = case_files("response");
    assert!(!files.is_empty(), "no response cases found");

    for path in files {
        let case = load_json(&path);
        let api = load_api(case["api"].as_str().unwrap());
        let resp_cfg = &api["response"];
        let response = &case["response"];
        let expect = &case["expect"];
        let nm = name_of(&path);

        let text_extract = resp_cfg.get("text_extract");
        let text = core::extract_text(response, text_extract);
        if Some(text.as_str()) != expect["text"].as_str() {
            failures.push(format!("{nm}: text mismatch (got {text:?})"));
        }

        if let Some(exp) = expect.get("activity") {
            let got = core::summarize_activity(response, resp_cfg.get("activity"));
            if Some(got.as_str()) != exp.as_str() {
                failures.push(format!("{nm}: activity mismatch (got {got:?})"));
            }
        }

        if let Some(exp) = expect.get("annotations_rendered") {
            let items = core::find_text_items(response, text_extract);
            let anns = core::extract_annotations(response, resp_cfg.get("annotations"), &items);
            let got = core::render_annotations(&anns, resp_cfg.get("annotations"));
            if Some(got.as_str()) != exp.as_str() {
                failures.push(format!("{nm}: annotations mismatch (got {got:?})"));
            }
        }

        if let Some(Value::Object(exp_meta)) = expect.get("metadata") {
            let mut meta = Map::new();
            if let Some(map) = resp_cfg.get("metadata").and_then(Value::as_object) {
                for (k, p) in map {
                    let v = p
                        .as_str()
                        .and_then(|p| core::get_path(response, p))
                        .cloned()
                        .unwrap_or(Value::Null);
                    meta.insert(k.clone(), v);
                }
            }
            if &meta != exp_meta {
                failures.push(format!(
                    "{nm}: metadata mismatch\n    got:      {}\n    expected: {}",
                    Value::Object(meta),
                    Value::Object(exp_meta.clone())
                ));
            }
        }

        if let Some(Value::Array(exp_arts)) = expect.get("artifacts") {
            let got = core::extract_artifacts(response, resp_cfg.get("artifacts"));
            if &got != exp_arts {
                failures.push(format!("{nm}: artifacts mismatch (got {got:?})"));
            }
        }
    }
    finish("response", failures);
}

#[test]
fn message_ir() {
    let mut failures = Vec::new();
    let files = case_files("message");
    assert!(!files.is_empty(), "no message cases found");

    for path in files {
        let case = load_json(&path);
        let nm = name_of(&path);

        let msg = if let Some(legacy) = case.get("legacy_payload") {
            core::normalize_prompt_payload(legacy)
        } else {
            core::message_from_dict(&case["message"])
        };

        if let Some(exp) = case.get("expect_message") {
            if &msg.to_value() != exp {
                failures.push(format!("{nm}: message mismatch (got {})", msg.to_value()));
            }
        }
        if let Some(exp) = case.get("expect_bindings") {
            match core::to_prompt_bindings(&msg) {
                Ok(b) if &b == exp => {}
                Ok(b) => failures.push(format!("{nm}: bindings mismatch (got {b})")),
                Err(e) => failures.push(format!("{nm}: lowering error: {e}")),
            }
        }
    }
    finish("message", failures);
}

#[test]
fn uploads() {
    let mut failures = Vec::new();
    let files = case_files("upload");
    assert!(!files.is_empty(), "no upload cases found");

    for path in files {
        let case = load_json(&path);
        let api = load_api(case["api"].as_str().unwrap());
        let nm = name_of(&path);
        let files_cfg = api.get("files");

        if let Some(exp) = case.get("expect_upload") {
            let got = core::read_upload(&case["response"], files_cfg);
            if &got != exp {
                failures.push(format!("{nm}: upload handle mismatch (got {got})"));
            }
        }
        if let Some(Value::String(exp_path)) = case.get("expect_upload_path") {
            let got = core::upload_path(files_cfg);
            if &got != exp_path {
                failures.push(format!("{nm}: upload path mismatch (got {got})"));
            }
        }
    }
    finish("upload", failures);
}

#[test]
fn operations() {
    let mut failures = Vec::new();
    let files = case_files("operation");
    assert!(!files.is_empty(), "no operation cases found");

    for path in files {
        let case = load_json(&path);
        let api = load_api(case["api"].as_str().unwrap());
        let nm = name_of(&path);
        let op = core::read_operation(&case["response"], api.get("operation"));

        if let Some(exp) = case.get("expect_operation") {
            if &op != exp {
                failures.push(format!("{nm}: operation mismatch (got {op})"));
            }
        }
        if let Some(Value::String(exp_path)) = case.get("expect_poll_path") {
            let id = op.get("id").cloned().unwrap_or(Value::Null);
            let got = core::poll_path(api.get("operation").unwrap(), &id);
            if &got != exp_path {
                failures.push(format!("{nm}: poll path mismatch (got {got})"));
            }
        }
    }
    finish("operation", failures);
}

#[test]
fn capabilities() {
    let mut failures = Vec::new();
    let files = case_files("capability");
    assert!(!files.is_empty(), "no capability cases found");

    for path in files {
        let case = load_json(&path);
        let api = load_api(case["api"].as_str().unwrap());
        let nm = name_of(&path);
        let cap = core::resolve_capabilities(&api, case.get("model_capabilities"));

        if let Some(exp) = case.get("expect_capabilities") {
            if &cap != exp {
                failures.push(format!("{nm}: capabilities mismatch (got {cap})"));
            }
        }
        if let Some(Value::Array(exp_errs)) = case.get("expect_errors") {
            let msg = core::message_from_dict(&case["message"]);
            let got: Vec<Value> = core::validate_message(&cap, &msg)
                .into_iter()
                .map(Value::from)
                .collect();
            if &got != exp_errs {
                failures.push(format!("{nm}: validation mismatch (got {got:?})"));
            }
        }
    }
    finish("capability", failures);
}
