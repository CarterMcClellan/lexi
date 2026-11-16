//! M4: `embed` (texts → vectors) and NVIDIA's image models on their own host,
//! both chosen by the provider's bindings and the model's tags.

mod common;

use common::{lexi, run, seeded_home, stderr, stdout};
use serde_json::{json, Value};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn go(home: &std::path::Path, args: &[&str]) -> std::process::Output {
    let mut cmd = lexi(home);
    cmd.args(args);
    run(cmd).await
}

async fn bodies(server: &MockServer, route: &str) -> Vec<Value> {
    server
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .filter(|r| r.url.path() == route)
        .map(|r| serde_json::from_slice(&r.body).unwrap())
        .collect()
}

fn write_config(home: &std::path::Path, yaml: &str) {
    std::fs::write(home.join("providers/providers-config.yaml"), yaml).unwrap();
}

async fn embeddings_server() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/embeddings"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "object": "list",
            "data": [
                {"object": "embedding", "index": 1, "embedding": [0.3, 0.4]},
                {"object": "embedding", "index": 0, "embedding": [0.1, 0.2]}
            ],
            "model": "m",
            "usage": {"prompt_tokens": 2, "total_tokens": 2}
        })))
        .mount(&server)
        .await;
    server
}

#[tokio::test]
async fn nvidia_embeddings_default_to_query_inputs() {
    let server = embeddings_server().await;
    let home = seeded_home();
    let h = home.path();
    write_config(
        h,
        &format!(
            "providers:\n  nvidia:\n    template: nvidia\n    url: {}\n    api_key: k\n",
            server.uri()
        ),
    );

    let o = go(
        h,
        &["--json", "embed", "nvidia", "first text", "second text"],
    )
    .await;
    assert!(o.status.success(), "{}", stderr(&o));
    let v: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["api"], "embeddings");
    assert_eq!(v["model"], "nvidia/nemotron-3-embed-1b");
    assert_eq!(
        (v["count"].clone(), v["dimensions"].clone()),
        (json!(2), json!(2))
    );
    assert_eq!(
        v["embeddings"],
        json!([[0.1, 0.2], [0.3, 0.4]]),
        "in input order"
    );
    let sent = &bodies(&server, "/embeddings").await[0];
    assert_eq!(sent["input"], json!(["first text", "second text"]));
    assert_eq!(sent["input_type"], "query", "the NVIDIA binding's default");

    // Schema flags from the embeddings API; --unset for models that reject a field.
    let o = go(h, &["embed", "nvidia", "doc", "--input-type", "passage"]).await;
    assert!(o.status.success(), "{}", stderr(&o));
    assert!(stdout(&o).starts_with("2 embeddings × 2 dimensions (nvidia/nemotron-3-embed-1b)"));
    assert_eq!(
        bodies(&server, "/embeddings").await[1]["input_type"],
        "passage"
    );
    let o = go(
        h,
        &[
            "embed",
            "nvidia",
            "x",
            "--model",
            "snowflake/arctic-embed-l",
            "--unset",
            "input_type",
        ],
    )
    .await;
    assert!(o.status.success(), "{}", stderr(&o));
    let sent = &bodies(&server, "/embeddings").await[2];
    assert_eq!(sent["model"], "snowflake/arctic-embed-l");
    assert!(sent.get("input_type").is_none());

    // A chat model can't embed; an embedding model can't chat.
    let o = go(
        h,
        &[
            "embed",
            "nvidia",
            "x",
            "--model",
            "meta/llama-3.1-70b-instruct",
        ],
    )
    .await;
    assert_eq!(o.status.code(), Some(1));
    assert!(
        stderr(&o).contains("doesn't produce vector"),
        "{}",
        stderr(&o)
    );
    let o = go(
        h,
        &["respond", "nvidia", "hi", "--model", "nvidia/embed-qa-4"],
    )
    .await;
    assert_eq!(o.status.code(), Some(1));
    assert!(
        stderr(&o).contains("kind:embedding model"),
        "{}",
        stderr(&o)
    );
}

#[tokio::test]
async fn openai_embeddings_take_dimensions() {
    let server = embeddings_server().await;
    let home = seeded_home();
    let h = home.path();
    write_config(
        h,
        &format!(
            "providers:\n  openai:\n    template: openai\n    url: {}\n    api_key: k\n",
            server.uri()
        ),
    );
    let o = go(
        h,
        &["--json", "embed", "openai", "hello", "--dimensions", "256"],
    )
    .await;
    assert!(o.status.success(), "{}", stderr(&o));
    let sent = &bodies(&server, "/embeddings").await[0];
    assert_eq!(sent["model"], "text-embedding-3-small");
    assert_eq!(sent["dimensions"], 256);
    assert!(
        sent.get("input_type").is_none(),
        "only NVIDIA's binding sets it"
    );

    let o = go(h, &["embed", "openai"]).await;
    assert_eq!(o.status.code(), Some(1));
    assert!(stderr(&o).contains("nothing to embed"), "{}", stderr(&o));
}

#[tokio::test]
async fn nvidia_images_use_their_own_host_and_a_model_path() {
    let chat = MockServer::start().await;
    let genai = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "artifacts": [{"base64": "iVBORyBmYWtl", "finishReason": "SUCCESS", "seed": 7}]
        })))
        .mount(&genai)
        .await;
    let home = seeded_home();
    let h = home.path();
    // Point the chat host and (via the binding merged by name) the image host at mocks.
    write_config(
        h,
        &format!(
            "providers:\n  nvidia:\n    template: nvidia\n    url: {}\n    api_key: k\n    apis:\n      - api: nvidia-genai-image\n        url: {}\n",
            chat.uri(),
            genai.uri()
        ),
    );
    let out = tempfile::tempdir().unwrap();
    let d = out.path().to_str().unwrap();

    let o = go(
        h,
        &[
            "respond", "nvidia", "a fox", "--output", "image", "--steps", "4", "--out", d,
        ],
    )
    .await;
    assert!(o.status.success(), "{}", stderr(&o));
    assert!(out.path().join("a-fox-1.png").exists());
    let sent = &bodies(&genai, "/genai/black-forest-labs/flux.1-schnell").await[0];
    assert_eq!(sent["prompt"], "a fox");
    assert_eq!(sent["steps"], 4);
    assert!(
        sent.get("model").is_none(),
        "the model is in the path, not the body"
    );
    assert!(chat.received_requests().await.unwrap().is_empty());

    // Another image model: its id becomes the path.
    let o = go(
        h,
        &[
            "respond",
            "nvidia",
            "a fox",
            "--model",
            "stabilityai/sdxl-turbo",
            "--out",
            d,
        ],
    )
    .await;
    assert!(o.status.success(), "{}", stderr(&o));
    assert_eq!(
        bodies(&genai, "/genai/stabilityai/sdxl-turbo").await.len(),
        1
    );

    let o = go(
        h,
        &[
            "respond", "nvidia", "a fox", "--output", "image", "--stream",
        ],
    )
    .await;
    assert!(
        stderr(&o).contains("nvidia-genai-image API doesn't stream"),
        "{}",
        stderr(&o)
    );
}
