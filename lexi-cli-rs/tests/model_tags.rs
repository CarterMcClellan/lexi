//! M1: provider templates, model tags and binding selection, end to end.
//! A templated `openai` provider is pointed at a mock server; its bindings
//! (Responses for chat, Images for `kind:image-generation`) route by tags.

mod common;

use common::{lexi, run, seeded_home, stderr, stdout};
use serde_json::{json, Value};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn templated_home(server: &MockServer) -> tempfile::TempDir {
    let home = seeded_home();
    std::fs::write(
        home.path().join("providers/providers-config.yaml"),
        format!(
            "providers:\n  openai:\n    template: openai\n    url: {}\n    api_key: k\n",
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

async fn sent(server: &MockServer, route: &str) -> Vec<Value> {
    server
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .filter(|r| r.url.path() == route)
        .map(|r| serde_json::from_slice(&r.body).unwrap())
        .collect()
}

async fn image_server() -> MockServer {
    let server = MockServer::start().await;
    let png = "iVBORyBmYWtl"; // base64 of b"\x89PNG fake"
    Mock::given(method("POST"))
        .and(path("/images/generations"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"data": [{"b64_json": png}]})),
        )
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/responses"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "output": [{"type": "message", "content": [{"type": "output_text", "text": "Hi."}]}]
        })))
        .mount(&server)
        .await;
    server
}

#[tokio::test]
async fn one_provider_serves_chat_and_images_by_tags() {
    let server = image_server().await;
    let home = templated_home(&server);
    let h = home.path();
    let out = tempfile::tempdir().unwrap();

    // Chat goes to the catch-all Responses binding.
    let o = go(h, &["respond", "openai", "--text", "hi"]).await;
    assert!(o.status.success(), "{}", stderr(&o));
    assert_eq!(stdout(&o), "Hi.\n");

    // `generate` picks the binding that produces images, and its default model.
    let dir = out.path().to_str().unwrap();
    let o = go(h, &["generate", "openai", "a fox", "--out", dir]).await;
    assert!(o.status.success(), "{}", stderr(&o));
    let body = &sent(&server, "/images/generations").await[0];
    assert_eq!(body["model"], "gpt-image-2.5-flare");
    assert_eq!(body["prompt"], "a fox");
    assert!(out.path().join("a-fox-1.png").exists());

    // An unconfigured model a tag rule recognizes is accepted, routed by its kind.
    let o = go(
        h,
        &[
            "generate",
            "openai",
            "x",
            "--model",
            "gpt-image-9",
            "--out",
            dir,
        ],
    )
    .await;
    assert!(o.status.success(), "{}", stderr(&o));
    assert_eq!(
        sent(&server, "/images/generations").await[1]["model"],
        "gpt-image-9"
    );

    // An image model can't answer a chat request — refused before any call.
    let o = go(
        h,
        &[
            "respond",
            "openai",
            "--text",
            "hi",
            "--model",
            "gpt-image-9",
        ],
    )
    .await;
    assert_eq!(o.status.code(), Some(1));
    assert!(
        stderr(&o).contains("kind:image-generation model (served by the image-generations API), which doesn't produce text"),
        "{}",
        stderr(&o)
    );
    assert_eq!(sent(&server, "/responses").await.len(), 1);

    // Typo protection stays for models nothing recognizes.
    let o = go(
        h,
        &["respond", "openai", "--text", "hi", "--model", "gpt-4oo"],
    )
    .await;
    assert!(
        stderr(&o).contains("not a configured model or alias"),
        "{}",
        stderr(&o)
    );
}

#[tokio::test]
async fn user_tags_route_unknown_models() {
    let server = image_server().await;
    let home = templated_home(&server);
    let h = home.path();
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path().to_str().unwrap();

    let o = go(
        h,
        &[
            "generate",
            "openai",
            "x",
            "--model",
            "vendor-img",
            "--out",
            d,
        ],
    )
    .await;
    assert_eq!(
        o.status.code(),
        Some(1),
        "unrecognized + unconfigured: refused"
    );

    let o = go(
        h,
        &[
            "models",
            "tag",
            "openai",
            "vendor-img",
            "+kind:image-generation",
        ],
    )
    .await;
    assert_eq!(stdout(&o), "vendor-img: +kind:image-generation\n");
    let o = go(
        h,
        &[
            "generate",
            "openai",
            "x",
            "--model",
            "vendor-img",
            "--out",
            d,
        ],
    )
    .await;
    assert!(o.status.success(), "{}", stderr(&o));
    assert_eq!(
        sent(&server, "/images/generations").await[0]["model"],
        "vendor-img"
    );
}

#[test]
fn tag_commands_and_vocabulary() {
    let home = seeded_home();
    let h = home.path();
    let json = |args: &[&str]| -> Value {
        let mut a = vec!["--json"];
        a.extend_from_slice(args);
        serde_json::from_slice(&common::lexi_out(h, &a).stdout).unwrap()
    };

    let cls = json(&[
        "models",
        "tags",
        "nvidia",
        "nvidia/nemotron-3.5-content-safety",
    ]);
    assert_eq!(cls["kind"], "classifier");
    assert!(cls["api"].is_null());
    assert!(cls["error"].as_str().unwrap().contains("no API configured"));

    let cls = json(&["models", "tags", "nvidia", "meta/llama-3.1-70b-instruct"]);
    let tags: Vec<&str> = cls["tags"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["tag"].as_str().unwrap())
        .collect();
    assert!(
        !tags.contains(&"in:image"),
        "the NVIDIA template starts text-only"
    );

    let v = json(&[
        "models",
        "tag",
        "nvidia",
        "meta/*",
        "+in:image",
        "label:big",
    ]);
    assert_eq!(v["tags"], json!(["+in:image", "+label:big"]));
    let cls = json(&["models", "tags", "nvidia", "meta/llama-3.1-70b-instruct"]);
    assert!(cls["tags"]
        .as_array()
        .unwrap()
        .contains(&json!({"tag": "in:image", "source": "user"})));
    assert_eq!(
        json(&["models", "tag", "nvidia", "meta/*", "--clear"])["cleared"],
        true
    );

    let o = common::lexi_out(h, &["models", "tag", "nvidia", "x", "out:smell"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(
        stderr(&o).starts_with("error: unknown tag 'out:smell'"),
        "{}",
        stderr(&o)
    );

    let vocab = json(&["tags"]);
    assert_eq!(
        vocab["kinds"]["image-generation"],
        json!({"in": ["text"], "out": ["image"]})
    );
    assert!(stdout(&common::lexi_out(h, &["tags"])).contains("kind:embedding"));

    // Configured models carry their classification; providers show their bindings.
    let models = json(&["models", "list", "openai"]);
    assert!(models
        .as_array()
        .unwrap()
        .iter()
        .all(|m| m["api"] == "responses"));
    let providers = json(&["providers", "list"]);
    let openai = providers
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "openai")
        .unwrap();
    assert_eq!(openai["template"], "openai");
    assert_eq!(openai["apis"], json!(["responses", "image-generations"]));
}
