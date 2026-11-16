//! `respond` / `api` edge cases ported from the Python CLI's test suite
//! (behaviors it asserted that the other Rust tests didn't).

mod common;

use common::{lexi, lexi_out, run, seeded_home, stderr, stdout};
use serde_json::{json, Value};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn write(home: &std::path::Path, rel: &str, text: &str) {
    let p = home.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

async fn messages_server() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "model": "m", "content": [{"type": "text", "text": "ok"}]
        })))
        .mount(&server)
        .await;
    server
}

/// Provider `p` on the Messages API at the mock, plus `extra` YAML lines.
fn home_for(server: &MockServer, extra: &str) -> tempfile::TempDir {
    let home = seeded_home();
    write(
        home.path(),
        "providers/providers-config.yaml",
        &format!(
            "providers:\n  p:\n    url: {}\n    api: messages\n    api_key: k\n    default_model: m\n{extra}",
            server.uri()
        ),
    );
    home
}

async fn go(home: &std::path::Path, args: &[&str]) -> std::process::Output {
    let mut cmd = lexi(home);
    cmd.args(args);
    run(cmd).await
}

async fn body(server: &MockServer) -> Value {
    let reqs = server.received_requests().await.unwrap();
    serde_json::from_slice(&reqs.last().unwrap().body).unwrap()
}

#[tokio::test]
async fn schema_validation_fails_before_sending_unless_disabled() {
    let server = messages_server().await;
    let home = home_for(&server, "");
    let o = go(
        home.path(),
        &["respond", "p", "--text", "x", "--temperature", "99"],
    )
    .await;
    assert_eq!(o.status.code(), Some(1), "{}", stdout(&o));
    assert!(
        server.received_requests().await.unwrap().is_empty(),
        "nothing sent"
    );

    let o = go(
        home.path(),
        &[
            "respond",
            "p",
            "--text",
            "x",
            "--temperature",
            "99",
            "--no-validate",
        ],
    )
    .await;
    assert!(o.status.success(), "{}", stderr(&o));
    assert_eq!(body(&server).await["temperature"], 99.0);
}

#[tokio::test]
async fn provider_overrides_merge_over_the_api_definition() {
    let server = messages_server().await;
    let home = home_for(
        &server,
        "    auth:\n      header: x-custom-key\n    tool_presets:\n      mine:\n        top_k: 3\n",
    );
    let o = go(
        home.path(),
        &["respond", "p", "--text", "x", "--tool", "mine,thinking_low"],
    )
    .await;
    assert!(o.status.success(), "{}", stderr(&o));
    let req = &server.received_requests().await.unwrap()[0];
    assert_eq!(req.headers.get("x-custom-key").unwrap(), "k");
    assert!(
        req.headers.get("anthropic-version").is_some(),
        "API's extra headers kept"
    );
    let b: Value = serde_json::from_slice(&req.body).unwrap();
    assert_eq!(b["top_k"], 3);
    assert!(
        b.get("thinking").is_some(),
        "shipped preset still available: {b}"
    );
}

#[tokio::test]
async fn inline_api_without_an_api_reference() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"out": "inline ok"})))
        .mount(&server)
        .await;
    let home = seeded_home();
    write(
        home.path(),
        "providers/providers-config.yaml",
        &format!(
            "providers:\n  p:\n    url: {}\n    api_key: k\n    default_model: m\n    \
             auth: {{header: Authorization, value_prefix: 'Bearer '}}\n    \
             request:\n      path: /chat\n      body_schema:\n        type: object\n        \
             properties:\n          model: {{type: string, x-bind: {{from: model}}}}\n          \
             input: {{type: string, x-bind: {{from: prompt}}}}\n    \
             response: {{text_extract: {{text_path: out}}}}\n",
            server.uri()
        ),
    );
    let o = go(home.path(), &["respond", "p", "--text", "hello"]).await;
    assert!(o.status.success(), "{}", stderr(&o));
    assert_eq!(stdout(&o), "inline ok\n");
    assert_eq!(body(&server).await, json!({"model": "m", "input": "hello"}));
}

#[tokio::test]
async fn flag_forms_and_missing_values() {
    let server = messages_server().await;
    let home = home_for(&server, "");
    let o = go(
        home.path(),
        &["respond", "p", "--text", "x", "-t", "0.4", "-v", "-r"],
    )
    .await;
    assert!(o.status.success(), "{}", stderr(&o));
    assert_eq!(body(&server).await["temperature"], 0.4);
    assert!(
        serde_json::from_slice::<Value>(&o.stdout).is_ok(),
        "-r prints the raw JSON"
    );

    let o = go(
        home.path(),
        &["respond", "p", "--text", "x", "--temperature"],
    )
    .await;
    assert_eq!(o.status.code(), Some(1));
    assert!(stderr(&o).contains("requires a value"));
}

#[tokio::test]
async fn params_files_json_missing_and_not_a_mapping() {
    let server = messages_server().await;
    let home = home_for(&server, "");
    let h = home.path();
    write(h, "p.json", r#"{"top_k": 7}"#);
    let p = h.join("p.json");
    let o = go(
        h,
        &[
            "respond",
            "p",
            "--text",
            "x",
            "--params",
            p.to_str().unwrap(),
        ],
    )
    .await;
    assert!(o.status.success(), "{}", stderr(&o));
    assert_eq!(body(&server).await["top_k"], 7);

    let o = go(h, &["respond", "p", "--text", "x", "--params", "nope.yaml"]).await;
    assert!(stderr(&o).contains("cannot read --params file"));
    for (name, text) in [("list.yaml", "- 1\n"), ("empty.yaml", "")] {
        write(h, name, text);
        let f = h.join(name);
        let o = go(
            h,
            &[
                "respond",
                "p",
                "--text",
                "x",
                "--params",
                f.to_str().unwrap(),
            ],
        )
        .await;
        assert_eq!(o.status.code(), Some(1), "{name}");
        assert!(
            stderr(&o).contains("must contain a mapping"),
            "{name}: {}",
            stderr(&o)
        );
    }
}

#[tokio::test]
async fn stream_raw_suppresses_text_and_cite_without_annotations_is_silent() {
    let server = MockServer::start().await;
    let completed = json!({"type": "response.completed", "response": {"status": "completed"}});
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(format!(
                    "event: response.output_text.delta\ndata: {{\"delta\":\"hi\"}}\n\n\
                     event: response.completed\ndata: {completed}\n\n"
                )),
        )
        .mount(&server)
        .await;
    let home = seeded_home();
    write(
        home.path(),
        "providers/providers-config.yaml",
        &format!("providers:\n  p:\n    url: {}\n    api: responses\n    api_key: k\n    default_model: m\n", server.uri()),
    );
    let o = go(
        home.path(),
        &["respond", "p", "--text", "x", "--stream", "-r"],
    )
    .await;
    assert!(o.status.success(), "{}", stderr(&o));
    let raw: Value = serde_json::from_slice(&o.stdout).expect("only the final JSON, no deltas");
    // `final_path` unwraps the event: raw is the Response object itself.
    assert_eq!(raw, json!({"status": "completed"}));

    // The Messages API (non-stream) has no annotations here: no Sources block.
    let server = messages_server().await;
    let home = home_for(&server, "");
    let o = go(home.path(), &["respond", "p", "--text", "x", "--cite"]).await;
    assert_eq!(stdout(&o), "ok\n");
}

#[test]
fn api_raw_output_preview_inputs_and_not_found() {
    let home = seeded_home();
    let h = home.path();
    let list: Value =
        serde_json::from_str(&stdout(&lexi_out(h, &["api", "list", "--raw"]))).unwrap();
    assert!(list.get("messages").is_some());
    let show: Value =
        serde_json::from_str(&stdout(&lexi_out(h, &["api", "show", "responses", "-r"]))).unwrap();
    assert_eq!(show["request"]["path"], "/responses");

    let o = lexi_out(
        h,
        &[
            "api",
            "preview",
            "chat-completions",
            "--prompt",
            "Hey",
            "--role",
            "system",
        ],
    );
    let v: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(
        v["body"]["messages"],
        json!([{"role": "system", "content": "Hey"}])
    );

    assert_eq!(
        lexi_out(h, &["api", "validate", "nope"]).status.code(),
        Some(1)
    );
    assert_eq!(
        lexi_out(h, &["api", "preview", "nope"]).status.code(),
        Some(1)
    );
}
