//! C5 `respond` parity: saved prompts, model resolution, schema flags, presets,
//! --set/--unset/--params, --raw/--verbose/--cite, streaming activity + final
//! response — asserted on the composed request body a mock provider receives.

mod common;

use common::{lexi, run, seeded_home, stderr, stdout};
use serde_json::{json, Value};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn write(home: &std::path::Path, rel: &str, text: &str) {
    let p = home.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

/// A seeded home with one provider `p` speaking `api` at the mock server, with
/// models `m-big` (default) and `m-small` (alias `small`), and an active prompt.
fn home_for(server: &MockServer, api: &str) -> tempfile::TempDir {
    let home = seeded_home();
    let h = home.path();
    write(
        h,
        "providers/providers-config.yaml",
        &format!(
            "providers:\n  p:\n    url: {}\n    api: {api}\n    api_key: k\n    default_model: from-config\n",
            server.uri()
        ),
    );
    write(
        h,
        "providers/p/configured-models.yaml",
        "models:\n  m-big:\n    name: Big\n  m-small:\n    name: Small\n    request:\n      path: /small\n",
    );
    write(
        h,
        "providers/p/default-model.yaml",
        "default_model: m-big\n",
    );
    write(
        h,
        "providers/p/model-aliases.yaml",
        "aliases:\n  small: m-small\n",
    );
    write(
        h,
        "prompts/$$.json",
        r#"{"prompt": "active text", "max_tokens": 256, "temperature": 0.7, "role": "user"}"#,
    );
    write(
        h,
        "prompts/named.json",
        r#"{"prompt": "named text", "role": "user"}"#,
    );
    home
}

async fn lexi_run(home: &std::path::Path, args: &[&str]) -> std::process::Output {
    let mut cmd = lexi(home);
    cmd.args(args);
    run(cmd).await
}

async fn sent_body(server: &MockServer, n: usize) -> Value {
    let reqs = server.received_requests().await.unwrap();
    serde_json::from_slice(&reqs[n].body).unwrap()
}

fn messages_reply() -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "model": "m-big",
        "content": [{"type": "text", "text": "Hi back."}],
        "usage": {"input_tokens": 3, "output_tokens": 2},
        "stop_reason": "end_turn"
    }))
}

#[tokio::test]
async fn sends_the_active_or_named_prompt_with_the_default_model() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/messages"))
        .respond_with(messages_reply())
        .mount(&server)
        .await;
    let home = home_for(&server, "messages");

    let o = lexi_run(home.path(), &["respond", "p"]).await;
    assert!(o.status.success(), "{}", stderr(&o));
    assert_eq!(stdout(&o), "Hi back.\n");
    let body = sent_body(&server, 0).await;
    assert_eq!(
        body["model"], "m-big",
        "default-model.yaml beats the config default"
    );
    assert_eq!(body["messages"][0]["content"][0]["text"], "active text");

    let o = lexi_run(home.path(), &["respond", "p", "named"]).await;
    assert!(o.status.success(), "{}", stderr(&o));
    assert_eq!(
        sent_body(&server, 1).await["messages"][0]["content"][0]["text"],
        "named text"
    );

    let o = lexi_run(home.path(), &["respond", "p", "missing"]).await;
    assert_eq!(o.status.code(), Some(1));
    assert!(stderr(&o).contains("prompt \"missing\" not found"));
}

#[tokio::test]
async fn model_aliases_and_per_model_overrides_resolve() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/small"))
        .respond_with(messages_reply())
        .mount(&server)
        .await;
    let home = home_for(&server, "messages");

    // `small` -> m-small, whose per-model `request.path` override applies.
    let o = lexi_run(home.path(), &["respond", "p", "--model", "small"]).await;
    assert!(o.status.success(), "{}", stderr(&o));
    assert_eq!(sent_body(&server, 0).await["model"], "m-small");

    let o = lexi_run(home.path(), &["respond", "p", "--model", "nope"]).await;
    assert_eq!(o.status.code(), Some(1));
    let err = stderr(&o);
    assert!(err.contains("not a configured model or alias"), "{err}");
    assert!(err.contains("available aliases: small"));
    assert!(err.contains("configured models: m-big, m-small"));
}

#[tokio::test]
async fn schema_flags_set_unset_params_and_text() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/messages"))
        .respond_with(messages_reply())
        .mount(&server)
        .await;
    let home = home_for(&server, "messages");
    let params = home.path().join("params.yaml");
    std::fs::write(&params, "top_p: 0.5\nsystem: be brief\n").unwrap();

    let o = lexi_run(
        home.path(),
        &[
            "respond",
            "p",
            "--text",
            "inline",
            "--role",
            "user",
            "--temperature",
            "0.3",
            "--max_tokens",
            "77",
            "--stop",
            "END",
            "--set",
            "top_k=5",
            "--params",
            params.to_str().unwrap(),
            "--unset",
            "system",
        ],
    )
    .await;
    assert!(o.status.success(), "{}", stderr(&o));
    let body = sent_body(&server, 0).await;
    assert_eq!(body["messages"][0]["content"][0]["text"], "inline");
    assert_eq!(body["temperature"], 0.3);
    assert_eq!(body["max_tokens"], 77);
    assert_eq!(body["stop_sequences"], json!(["END"]));
    assert_eq!(body["top_k"], 5);
    assert_eq!(body["top_p"], 0.5);
    assert!(body.get("system").is_none(), "--unset beats --params");

    let o = lexi_run(home.path(), &["respond", "p", "--temperature", "hot"]).await;
    assert_eq!(o.status.code(), Some(1));
    assert!(stderr(&o).contains("expected number"));
    let o = lexi_run(home.path(), &["respond", "p", "--bogus"]).await;
    assert!(stderr(&o).contains("unknown option \"--bogus\""));
    let o = lexi_run(home.path(), &["respond", "p", "--tool", "web_search"]).await;
    assert!(stderr(&o).contains("unknown preset"));
}

#[tokio::test]
async fn raw_verbose_and_json_output() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/messages"))
        .respond_with(messages_reply())
        .mount(&server)
        .await;
    let home = home_for(&server, "messages");

    let o = lexi_run(home.path(), &["respond", "p", "-r"]).await;
    let raw: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(raw["stop_reason"], "end_turn");

    let o = lexi_run(home.path(), &["respond", "p", "--verbose"]).await;
    assert_eq!(stdout(&o), "Hi back.\n");
    assert_eq!(stderr(&o), "[m-big | 3 in + 2 out | end_turn]\n");

    let o = lexi_run(home.path(), &["respond", "p", "--json"]).await;
    let v: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["text"], "Hi back.");
    assert_eq!(v["model"], "m-big");
    assert_eq!(v["metadata"]["input_tokens"], 3);
}

#[tokio::test]
async fn responses_api_cite_presets_and_streaming_with_activity() {
    let server = MockServer::start().await;
    let response = json!({
        "model": "m-big",
        "status": "completed",
        "usage": {"input_tokens": 1, "output_tokens": 1, "total_tokens": 2},
        "output": [{"type": "message", "content": [{
            "type": "output_text",
            "text": "Answer.",
            "annotations": [{"type": "url_citation", "url": "https://a.example", "title": "A"}]
        }]}]
    });
    Mock::given(method("POST"))
        .and(path("/responses"))
        .respond_with(ResponseTemplate::new(200).set_body_json(response.clone()))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    let completed = json!({"type": "response.completed", "response": response});
    let sse = format!(
        "event: response.web_search_call.searching\ndata: {{}}\n\n\
         event: response.output_text.delta\ndata: {{\"delta\":\"Ans\"}}\n\n\
         event: response.output_text.delta\ndata: {{\"delta\":\"wer.\"}}\n\n\
         event: response.completed\ndata: {completed}\n\n"
    );
    Mock::given(method("POST"))
        .and(path("/responses"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(sse),
        )
        .mount(&server)
        .await;
    let home = home_for(&server, "responses");

    let o = lexi_run(
        home.path(),
        &["respond", "p", "--cite", "--tool", "web_search"],
    )
    .await;
    assert!(o.status.success(), "{}", stderr(&o));
    assert_eq!(
        stdout(&o),
        "Answer.\n\nSources:\n  [1] https://a.example — A\n"
    );
    let body = sent_body(&server, 0).await;
    assert!(body["tools"].to_string().contains("web_search"), "{body}");

    // Streaming: deltas on stdout, the activity label on stderr; the final
    // response event feeds --verbose.
    let o = lexi_run(home.path(), &["respond", "p", "--stream", "--verbose"]).await;
    assert!(o.status.success(), "{}", stderr(&o));
    assert_eq!(stdout(&o), "Answer.\n");
    assert!(stderr(&o).contains("searching the web"), "{}", stderr(&o));
    assert_eq!(sent_body(&server, 1).await["stream"], true);

    let o = lexi_run(
        home.path(),
        &["--json", "respond", "p", "--stream", "--cite"],
    )
    .await;
    let events: Vec<Value> = stdout(&o)
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(
        events[0],
        json!({"event": "provider_start", "provider": "p"})
    );
    assert_eq!(events[1]["event"], "activity");
    assert_eq!(
        events[2],
        json!({"event": "delta", "provider": "p", "text": "Ans"})
    );
    let done = events.last().unwrap();
    assert_eq!(done["event"], "provider_done");
    assert_eq!(done["text"], "Answer.");
    // Metadata and citations come from the final event's Response object.
    assert_eq!(done["metadata"]["total_tokens"], 2);
    assert_eq!(done["sources"], json!(["[1] https://a.example — A"]));
}

#[tokio::test]
async fn api_preview_and_save() {
    let home = seeded_home();
    let h = home.path();
    let o = lexi_run(h, &["api", "preview", "messages", "--model", "mx"]).await;
    let v: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["valid"], true);
    assert_eq!(v["body"]["model"], "mx");
    assert_eq!(v["presets"], json!(["thinking_high", "thinking_low"]));

    let file = h.join("mine.yaml");
    std::fs::write(&file, "name: mine\nrequest: {}\n").unwrap();
    let o = lexi_run(h, &["api", "preview", "--file", file.to_str().unwrap()]).await;
    let v: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["valid"], false);

    let o = lexi_run(h, &["api", "save", "mine", file.to_str().unwrap()]).await;
    assert!(o.status.success());
    assert!(stdout(&o).contains("saved, with validation errors:"));
    assert!(h.join("apis/mine.yaml").exists());
    let o = lexi_run(h, &["api", "save", "messages", file.to_str().unwrap()]).await;
    assert_eq!(o.status.code(), Some(1));
}
