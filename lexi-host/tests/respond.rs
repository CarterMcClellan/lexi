//! End-to-end text chat against a mock HTTP server (no live provider).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use lexi_host::config;
use lexi_host::respond::{
    respond_background_with_effective, respond_stream_with_effective, respond_with_effective,
};
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
    let res = respond_with_effective(
        &client,
        &eff,
        "test-key",
        "Hi there",
        "user",
        "claude-x",
        &[],
    )
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
    let err = respond_with_effective(&client, &eff, "bad", "hi", "user", "claude-x", &[])
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        lexi_host::HostError::HttpStatus { status: 401, .. }
    ));
}

#[tokio::test]
async fn text_chat_streaming() {
    let server = MockServer::start().await;

    // An Anthropic-style SSE body: deltas then message_stop.
    let sse = concat!(
        "event: message_start\ndata: {\"type\":\"message_start\"}\n\n",
        "event: content_block_delta\ndata: {\"delta\":{\"text\":\"Hello\"}}\n\n",
        "event: content_block_delta\ndata: {\"delta\":{\"text\":\" world\"}}\n\n",
        "event: message_stop\ndata: {}\n\n",
    );
    Mock::given(method("POST"))
        .and(path("/messages"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(sse),
        )
        .mount(&server)
        .await;

    let mut eff = config::load_api_def("messages").unwrap();
    eff["url"] = Value::from(server.uri());

    let deltas = Arc::new(Mutex::new(Vec::<String>::new()));
    let sink = deltas.clone();
    let client = reqwest::Client::new();
    let res =
        respond_stream_with_effective(&client, &eff, "k", "Hi", "user", "claude-x", &[], |d| {
            sink.lock().unwrap().push(d.to_string());
        })
        .await
        .expect("stream ok");

    assert_eq!(res.text, "Hello world");
    assert_eq!(*deltas.lock().unwrap(), vec!["Hello", " world"]);

    // The request opted into streaming.
    let received = &server.received_requests().await.unwrap()[0];
    let sent: Value = serde_json::from_slice(&received.body).unwrap();
    assert_eq!(sent["stream"], true);
}

#[tokio::test]
async fn background_submit_poll_fetch() {
    let server = MockServer::start().await;

    // Submit: a background response comes back queued (pending, not done).
    Mock::given(method("POST"))
        .and(path("/responses"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"id": "resp_1", "status": "queued"})),
        )
        .mount(&server)
        .await;

    // Poll: completed, with the final output.
    Mock::given(method("GET"))
        .and(path("/responses/resp_1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "resp_1",
            "status": "completed",
            "output": [
                {"type": "message", "content": [{"type": "output_text", "text": "Background done."}]}
            ],
            "usage": {"input_tokens": 1, "output_tokens": 2, "total_tokens": 3}
        })))
        .mount(&server)
        .await;

    let mut eff = config::load_api_def("responses").unwrap();
    eff["url"] = Value::from(server.uri());

    let client = reqwest::Client::new();
    let res = respond_background_with_effective(
        &client,
        &eff,
        "k",
        "Hi",
        "user",
        "gpt-4o",
        Duration::from_millis(5),
        10,
    )
    .await
    .expect("background ok");

    assert_eq!(res.text, "Background done.");

    // Exactly two requests: submit (with background: true) + one poll.
    let reqs = server.received_requests().await.unwrap();
    assert_eq!(reqs.len(), 2);
    let submit: Value = serde_json::from_slice(&reqs[0].body).unwrap();
    assert_eq!(submit["background"], true);
    assert_eq!(reqs[1].url.path(), "/responses/resp_1");
}

#[tokio::test]
async fn text_plus_image_url() {
    use lexi_host::image::ResolvedImage;

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "content": [{"type": "text", "text": "A cat."}],
            "stop_reason": "end_turn"
        })))
        .mount(&server)
        .await;

    let mut eff = config::load_api_def("messages").unwrap();
    eff["url"] = Value::from(server.uri());

    let images = vec![ResolvedImage::Url("https://example.com/cat.jpg".into())];
    let client = reqwest::Client::new();
    respond_with_effective(&client, &eff, "k", "Describe", "user", "claude-x", &images)
        .await
        .expect("respond ok");

    // The composed message carries a text part + an Anthropic image(url) block.
    let sent: Value =
        serde_json::from_slice(&server.received_requests().await.unwrap()[0].body).unwrap();
    assert_eq!(
        sent["messages"][0]["content"],
        json!([
            {"type": "text", "text": "Describe"},
            {"type": "image", "source": {"type": "url", "url": "https://example.com/cat.jpg"}}
        ])
    );
}
