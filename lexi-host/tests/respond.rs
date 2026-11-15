//! End-to-end text chat against a mock HTTP server (no live provider).

use lexi_host::{config, respond::respond_with_effective};
use serde_json::{json, Value};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn text_chat_end_to_end() {
    let server = MockServer::start().await;

    // Anthropic Messages-shaped response; assert the composed request too.
    Mock::given(method("POST"))
        .and(path("/messages"))
        .and(header("x-api-key", "test-key"))
        .and(header("anthropic-version", "2023-06-01"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "content": [
                {"type": "text", "text": "Hello from "},
                {"type": "text", "text": "the mock."}
            ],
            "usage": {"input_tokens": 1, "output_tokens": 3},
            "stop_reason": "end_turn"
        })))
        .mount(&server)
        .await;

    let mut eff = config::load_api_def("messages").unwrap();
    eff["url"] = Value::from(server.uri());

    let client = reqwest::Client::new();
    let res = respond_with_effective(&client, &eff, "test-key", "Hi there", "user", "claude-x")
        .await
        .expect("respond ok");

    // Text extraction walks content[] and concatenates text blocks.
    assert_eq!(res.text, "Hello from the mock.");

    // The recorded request body was composed by the engine.
    let received = &server.received_requests().await.unwrap()[0];
    let sent: Value = serde_json::from_slice(&received.body).unwrap();
    assert_eq!(sent["model"], "claude-x");
    assert_eq!(sent["max_tokens"], 4096);
    assert_eq!(
        sent["messages"],
        json!([{"role": "user", "content": [{"type": "text", "text": "Hi there"}]}])
    );
}

#[tokio::test]
async fn http_error_surfaces() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(401).set_body_string("unauthorized"))
        .mount(&server)
        .await;

    let mut eff = config::load_api_def("messages").unwrap();
    eff["url"] = Value::from(server.uri());

    let client = reqwest::Client::new();
    let err = respond_with_effective(&client, &eff, "bad", "hi", "user", "claude-x")
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        lexi_host::HostError::HttpStatus { status: 401, .. }
    ));
}
