//! M2: one `respond` for every output — text, images, or both — with
//! `--output`, `--out`, never-overwriting artifact files, and schema flags from
//! the API the request resolves to.

mod common;

use common::{lexi, run, seeded_home, stderr, stdout};
use serde_json::{json, Value};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const PNG_B64: &str = "iVBORyBmYWtl"; // b"\x89PNG fake"

async fn server() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/images/generations"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [{"b64_json": PNG_B64, "revised_prompt": "a watercolor fox, soft light"}]
        })))
        .mount(&server)
        .await;
    // A Responses reply carrying both text and an image_generation tool image.
    Mock::given(method("POST"))
        .and(path("/responses"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "output": [
                {"type": "image_generation_call", "status": "completed", "result": PNG_B64},
                {"type": "message", "content": [{"type": "output_text", "text": "Here's your fox."}]}
            ]
        })))
        .mount(&server)
        .await;
    server
}

fn home(server: &MockServer) -> tempfile::TempDir {
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

#[tokio::test]
async fn output_image_saves_files_without_overwriting() {
    let server = server().await;
    let home = home(&server);
    let h = home.path();
    let out = tempfile::tempdir().unwrap();
    let d = out.path().to_str().unwrap();

    let args = [
        "respond",
        "openai",
        "A",
        "watercolor",
        "fox!",
        "--output",
        "image",
        "--out",
        d,
    ];
    let o = go(h, &args).await;
    assert!(o.status.success(), "{}", stderr(&o));
    let first = out.path().join("a-watercolor-fox-1.png");
    assert_eq!(stdout(&o), format!("Saved image: {}\n", first.display()));
    assert_eq!(std::fs::read(&first).unwrap(), b"\x89PNG fake");

    // Same prompt again: a new file, the first is untouched.
    let o = go(h, &args).await;
    assert!(o.status.success(), "{}", stderr(&o));
    assert!(out.path().join("a-watercolor-fox-2.png").exists());

    let body = &bodies(&server, "/images/generations").await[0];
    assert_eq!(body["prompt"], "A watercolor fox!");
    assert_eq!(body["model"], "gpt-image-2.5-flare");
}

#[tokio::test]
async fn schema_flags_come_from_the_selected_api() {
    let server = server().await;
    let home = home(&server);
    let h = home.path();
    let out = tempfile::tempdir().unwrap();
    let d = out.path().to_str().unwrap();

    // --size / --quality exist on the Images API...
    let o = go(
        h,
        &[
            "respond",
            "openai",
            "fox",
            "--output",
            "image",
            "--size",
            "1536x864",
            "--quality",
            "low",
            "--out",
            d,
        ],
    )
    .await;
    assert!(o.status.success(), "{}", stderr(&o));
    let body = &bodies(&server, "/images/generations").await[0];
    assert_eq!(body["size"], "1536x864");
    assert_eq!(body["quality"], "low");

    // ...not on the chat API.
    let o = go(h, &["respond", "openai", "fox", "--size", "1536x864"]).await;
    assert_eq!(o.status.code(), Some(1));
    assert!(
        stderr(&o).contains("unknown option \"--size\""),
        "{}",
        stderr(&o)
    );

    // The Images API doesn't stream: refused up front.
    let o = go(
        h,
        &["respond", "openai", "fox", "--output", "image", "--stream"],
    )
    .await;
    assert_eq!(o.status.code(), Some(1));
    assert!(
        stderr(&o).contains("image-generations API doesn't stream"),
        "{}",
        stderr(&o)
    );
}

#[tokio::test]
async fn one_reply_with_text_and_an_image() {
    let server = server().await;
    let home = home(&server);
    let h = home.path();
    let out = tempfile::tempdir().unwrap();
    let d = out.path().to_str().unwrap();

    let o = go(
        h,
        &[
            "respond",
            "openai",
            "draw a fox",
            "--tool",
            "image_generation",
            "--out",
            d,
        ],
    )
    .await;
    assert!(o.status.success(), "{}", stderr(&o));
    let saved = out.path().join("draw-a-fox-1.png");
    assert_eq!(
        stdout(&o),
        format!("Here's your fox.\nSaved image: {}\n", saved.display())
    );
    let body = &bodies(&server, "/responses").await[0];
    assert_eq!(body["tools"], json!([{"type": "image_generation"}]));

    let o = go(
        h,
        &[
            "--json",
            "respond",
            "openai",
            "draw a fox",
            "--tool",
            "image_generation",
            "--out",
            d,
        ],
    )
    .await;
    let v: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["api"], "responses");
    assert_eq!(v["text"], "Here's your fox.");
    let art = &v["artifacts"][0];
    assert_eq!(art["type"], "image");
    assert_eq!(art["media_type"], "image/png");
    assert!(art["path"].as_str().unwrap().ends_with("draw-a-fox-2.png"));
}

#[tokio::test]
async fn generate_is_respond_output_image() {
    let server = server().await;
    let home = home(&server);
    let out = tempfile::tempdir().unwrap();
    let d = out.path().to_str().unwrap();

    let o = go(
        home.path(),
        &["--json", "generate", "openai", "a fox", "--out", d],
    )
    .await;
    assert!(o.status.success(), "{}", stderr(&o));
    let v: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["api"], "image-generations");
    assert_eq!(
        v["artifacts"][0]["revised_prompt"],
        "a watercolor fox, soft light"
    );
    assert!(out.path().join("a-fox-1.png").exists());
}
