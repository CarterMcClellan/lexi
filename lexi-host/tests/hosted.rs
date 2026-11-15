//! Hosted-model discovery against a mock HTTP server.

use lexi_host::fetch_hosted_models_with;
use serde_json::{json, Value};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn fetches_and_normalizes_hosted_models() {
    let server = MockServer::start().await;

    // The `/v1/messages` url normalizes to a `/v1/models` endpoint, and the
    // request carries the configured auth header.
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .and(header("authorization", "Bearer k"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "object": "list",
            "data": [{ "id": "gpt-4o" }, { "id": "gpt-4o-mini" }]
        })))
        .mount(&server)
        .await;

    let eff = json!({
        "url": format!("{}/v1/messages", server.uri()),
        "hosted_models": "/models",
        "auth": { "header": "Authorization", "value_prefix": "Bearer " }
    });

    let client = reqwest::Client::new();
    let models = fetch_hosted_models_with(&client, &eff, "k")
        .await
        .expect("hosted ok");

    assert_eq!(models.len(), 2);
    assert_eq!(models[0]["id"], "gpt-4o");
    assert_eq!(models[1]["id"], "gpt-4o-mini");
}

#[tokio::test]
async fn surfaces_http_errors() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/models"))
        .respond_with(ResponseTemplate::new(401).set_body_string("bad key"))
        .mount(&server)
        .await;

    let eff: Value = json!({ "url": server.uri(), "hosted_models": "/models" });
    let client = reqwest::Client::new();
    let err = fetch_hosted_models_with(&client, &eff, "k")
        .await
        .unwrap_err();
    assert!(err.to_string().contains("401"));
}
