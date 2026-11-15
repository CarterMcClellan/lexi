//! Files-API multipart upload against a mock HTTP server.

use lexi_host::{config, upload::upload_file_with_effective};
use serde_json::{json, Value};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn upload_returns_file_id() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/files"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "file-abc123",
            "object": "file",
            "purpose": "vision"
        })))
        .mount(&server)
        .await;

    // responses.yaml defines a `files` block (upload path /files, id_path id).
    let mut eff = config::load_api_def("responses").unwrap();
    eff["url"] = Value::from(server.uri());

    // A small local file to upload.
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("cat.png");
    std::fs::write(&file, b"\x89PNG bytes").unwrap();

    let client = reqwest::Client::new();
    let id = upload_file_with_effective(&client, &eff, "k", &file)
        .await
        .expect("upload ok");
    assert_eq!(id, "file-abc123");

    // The request was multipart (not application/json).
    let received = &server.received_requests().await.unwrap()[0];
    let ct = received
        .headers
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(
        ct.starts_with("multipart/form-data"),
        "content-type was {ct}"
    );
}
