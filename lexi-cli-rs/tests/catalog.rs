//! M3: the model catalog — paginated listing, cache, catalog-derived tags and
//! limits, `models hosted --tag/--cached`, and checks before sending.

mod common;

use common::{lexi, run, seeded_home, stderr, stdout};
use serde_json::{json, Value};
use wiremock::matchers::{method, path, query_param, query_param_is_missing};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn caps(image: bool, pdf: bool) -> Value {
    json!({
        "image_input": {"supported": image},
        "pdf_input": {"supported": pdf},
        "thinking": {"supported": true},
        "structured_outputs": {"supported": false}
    })
}

/// An Anthropic-shaped catalog over two pages, plus a Messages endpoint.
async fn anthropic_server() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/models"))
        .and(query_param("limit", "1000"))
        .and(query_param_is_missing("after_id"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [
                {"id": "claude-a", "display_name": "Claude A", "max_input_tokens": 200000,
                 "max_tokens": 1000, "capabilities": caps(true, true)},
                {"id": "claude-b", "display_name": "Claude B", "max_input_tokens": 100000,
                 "max_tokens": 8000, "capabilities": caps(false, false)}
            ],
            "has_more": true, "last_id": "claude-b"
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/models"))
        .and(query_param("after_id", "claude-b"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [{"id": "claude-c", "display_name": "Claude C", "max_tokens": 4000,
                      "capabilities": caps(true, false)}],
            "has_more": false, "last_id": "claude-c"
        })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "content": [{"type": "text", "text": "ok"}]
        })))
        .mount(&server)
        .await;
    server
}

fn home(provider: &str, server: &MockServer) -> tempfile::TempDir {
    let home = seeded_home();
    std::fs::write(
        home.path().join("providers/providers-config.yaml"),
        format!(
            "providers:\n  {provider}:\n    template: {provider}\n    url: {}\n    api_key: k\n",
            server.uri()
        ),
    )
    .unwrap();
    home
}

async fn go(home: &std::path::Path, args: &[&str]) -> std::process::Output {
    let mut cmd = lexi(home);
    cmd.args(args);
    run(cmd).await
}

fn json_of(o: &std::process::Output) -> Value {
    serde_json::from_slice(&o.stdout).unwrap_or_else(|_| panic!("not JSON: {}", stdout(o)))
}

async fn posts(s: &MockServer) -> usize {
    s.received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.url.path() == "/messages")
        .count()
}

fn ids(v: &Value) -> Vec<&str> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].as_str().unwrap())
        .collect()
}

#[tokio::test]
async fn hosted_listing_pages_caches_and_classifies() {
    let server = anthropic_server().await;
    let home = home("anthropic", &server);
    let h = home.path();

    let o = go(h, &["--json", "models", "hosted", "anthropic"]).await;
    assert!(o.status.success(), "{}", stderr(&o));
    let v = json_of(&o);
    assert_eq!(ids(&v), ["claude-a", "claude-b", "claude-c"], "both pages");
    let a = &v[0];
    assert_eq!(a["kind"], "chat");
    assert_eq!(a["api"], "messages");
    let tags: Vec<&str> = a["tags"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert!(tags.contains(&"in:image") && tags.contains(&"in:document"));
    assert!(tags.contains(&"feature:thinking"));
    assert!(
        !tags.contains(&"feature:structured-output"),
        "supported: false removes"
    );
    assert!(h.join("providers/anthropic/catalog.json").exists());

    // Filtered, from the cache (no new listing request).
    let before = server.received_requests().await.unwrap().len();
    let o = go(
        h,
        &[
            "--json",
            "models",
            "hosted",
            "anthropic",
            "--cached",
            "--tag",
            "in:image",
        ],
    )
    .await;
    assert!(o.status.success(), "{}", stderr(&o));
    assert_eq!(ids(&json_of(&o)), ["claude-a", "claude-c"]);
    assert_eq!(server.received_requests().await.unwrap().len(), before);

    // Human output gains kind / api columns.
    let o = go(h, &["models", "hosted", "anthropic", "--cached"]).await;
    assert!(stdout(&o).contains("id=claude-b, display_name=Claude B, max_input_tokens=100000, max_tokens=8000, kind=chat, api=messages"), "{}", stdout(&o));

    // Classification explains where each tag came from.
    let cls = json_of(&go(h, &["--json", "models", "tags", "anthropic", "claude-b"]).await);
    let tags = cls["tags"].as_array().unwrap();
    assert!(
        !tags.iter().any(|t| t["tag"] == "in:image"),
        "catalog said no"
    );
    assert!(tags.contains(&json!({"tag": "feature:thinking", "source": "catalog"})));
    assert_eq!(
        cls["limits"],
        json!({"context_window": 100000, "max_output_tokens": 8000})
    );
    assert_eq!(cls["name"], "Claude B");

    let o = go(
        h,
        &[
            "models",
            "hosted",
            "anthropic",
            "--cached",
            "--tag",
            "out:smell",
        ],
    )
    .await;
    assert_eq!(o.status.code(), Some(1));
}

#[tokio::test]
async fn requests_are_checked_against_the_model_before_sending() {
    let server = anthropic_server().await;
    let home = home("anthropic", &server);
    let h = home.path();
    assert!(go(h, &["models", "hosted", "anthropic"])
        .await
        .status
        .success());
    let img = h.join("cat.png");
    std::fs::write(&img, b"png").unwrap();
    let img = img.to_str().unwrap();
    // A catalog model that isn't configured is accepted.
    // (The schema's default max_tokens, 4096, is lowered to its 4000 limit.)
    let o = go(h, &["respond", "anthropic", "hi", "--model", "claude-c"]).await;
    assert!(o.status.success(), "{}", stderr(&o));
    assert_eq!(posts(&server).await, 1);
    let sent: Value = serde_json::from_slice(
        &server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .rfind(|r| r.url.path() == "/messages")
            .unwrap()
            .body,
    )
    .unwrap();
    assert_eq!(sent["max_tokens"], 4000);

    // The catalog says claude-b takes no images: refused, nothing sent...
    let o = go(
        h,
        &[
            "respond",
            "anthropic",
            "what is this",
            "--model",
            "claude-b",
            "--image",
            img,
        ],
    )
    .await;
    assert_eq!(o.status.code(), Some(1));
    assert!(
        stderr(&o).contains("claude-b doesn't accept image input"),
        "{}",
        stderr(&o)
    );
    assert_eq!(posts(&server).await, 1);
    // ...unless told not to check.
    let o = go(
        h,
        &[
            "respond",
            "anthropic",
            "what is this",
            "--model",
            "claude-b",
            "--image",
            img,
            "--no-validate",
        ],
    )
    .await;
    assert!(o.status.success(), "{}", stderr(&o));
    assert_eq!(posts(&server).await, 2);

    // max_tokens over the model's limit (claude-a: 1000).
    let o = go(
        h,
        &[
            "respond",
            "anthropic",
            "hi",
            "--model",
            "claude-a",
            "--max-tokens",
            "5000",
        ],
    )
    .await;
    assert_eq!(o.status.code(), Some(1));
    assert!(
        stderr(&o).contains("max_tokens 5000 is more than claude-a allows (1000)"),
        "{}",
        stderr(&o)
    );
    let o = go(
        h,
        &[
            "respond",
            "anthropic",
            "hi",
            "--model",
            "claude-a",
            "--max-tokens",
            "900",
        ],
    )
    .await;
    assert!(o.status.success(), "{}", stderr(&o));
}

#[tokio::test]
async fn deprecated_models_warn() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [
                {"id": "gpt-old", "object": "model", "created": 1, "owned_by": "system", "shutdown_date": "2026-10-23"},
                {"id": "gpt-new", "object": "model", "created": 2, "owned_by": "system", "shutdown_date": null}
            ]
        })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/responses"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "output": [{"type": "message", "content": [{"type": "output_text", "text": "ok"}]}]
        })))
        .mount(&server)
        .await;
    let home = home("openai", &server);
    let h = home.path();
    let o = go(
        h,
        &[
            "--json",
            "models",
            "hosted",
            "openai",
            "--tag",
            "status:deprecated",
        ],
    )
    .await;
    assert_eq!(ids(&json_of(&o)), ["gpt-old"]);

    let o = go(h, &["respond", "openai", "hi", "--model", "gpt-old"]).await;
    assert!(o.status.success(), "{}", stderr(&o));
    assert!(
        stderr(&o).contains("warning: gpt-old is deprecated"),
        "{}",
        stderr(&o)
    );
    let o = go(
        h,
        &["--json", "respond", "openai", "hi", "--model", "gpt-old"],
    )
    .await;
    assert!(json_of(&o)["warnings"][0]
        .as_str()
        .unwrap()
        .contains("deprecated"));
    let o = go(h, &["respond", "openai", "hi", "--model", "gpt-new"]).await;
    assert!(!stderr(&o).contains("warning"), "{}", stderr(&o));
}
