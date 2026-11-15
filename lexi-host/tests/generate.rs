//! Image generation → artifact materialization, against a mock HTTP server.

use base64::prelude::*;
use lexi_host::{config, generate::generate_images_with_effective};
use serde_json::{json, Value};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn image_gen_eff(uri: String) -> Value {
    let mut eff = config::load_api_def("image-generations").unwrap();
    eff["url"] = Value::from(uri);
    eff
}

#[tokio::test]
async fn generate_saves_base64_artifact() {
    let server = MockServer::start().await;
    let payload = b"\x89PNG fake bytes";
    let b64 = BASE64_STANDARD.encode(payload);

    Mock::given(method("POST"))
        .and(path("/images/generations"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "created": 1,
            "data": [{ "b64_json": b64, "revised_prompt": "a refined cat" }]
        })))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let client = reqwest::Client::new();
    let saved = generate_images_with_effective(
        &client,
        &image_gen_eff(server.uri()),
        "k",
        "a cat",
        "gpt-image-1",
        dir.path(),
    )
    .await
    .expect("generate ok");

    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].revised_prompt.as_deref(), Some("a refined cat"));
    assert_eq!(std::fs::read(&saved[0].path).unwrap(), payload);
    assert_eq!(saved[0].path.extension().unwrap(), "png");
}

#[tokio::test]
async fn generate_downloads_url_artifact() {
    let server = MockServer::start().await;
    let payload = b"jpeg-bytes-here";

    // The generation response points at an (absolute) URL the host then fetches.
    Mock::given(method("POST"))
        .and(path("/images/generations"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "created": 1,
            "data": [{ "url": format!("{}/files/out.jpg", server.uri()) }]
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/files/out.jpg"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(payload.to_vec()))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let client = reqwest::Client::new();
    let saved = generate_images_with_effective(
        &client,
        &image_gen_eff(server.uri()),
        "k",
        "a dog",
        "dall-e-3",
        dir.path(),
    )
    .await
    .expect("generate ok");

    assert_eq!(saved.len(), 1);
    assert_eq!(std::fs::read(&saved[0].path).unwrap(), payload);
    assert_eq!(saved[0].path.extension().unwrap(), "jpg");
}
