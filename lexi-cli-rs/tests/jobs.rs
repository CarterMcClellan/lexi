//! C7 detached background jobs: `respond --background [--detach]`, `jobs`.

mod common;

use common::{lexi, run, seeded_home, stderr, stdout};
use serde_json::{json, Value};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn completed() -> Value {
    json!({
        "id": "resp_1", "status": "completed",
        "output": [{"type": "message", "content": [{"type": "output_text", "text": "Done!"}]}]
    })
}

async fn server_with_job() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/responses"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"id": "resp_1", "status": "queued"})),
        )
        .mount(&server)
        .await;
    // First poll: still running; afterwards: completed.
    Mock::given(method("GET"))
        .and(path("/responses/resp_1"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"id": "resp_1", "status": "in_progress"})),
        )
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/responses/resp_1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(completed()))
        .mount(&server)
        .await;
    server
}

fn home_for(server: &MockServer) -> tempfile::TempDir {
    let home = seeded_home();
    std::fs::write(
        home.path().join("providers/providers-config.yaml"),
        format!(
            "providers:\n  p:\n    url: {}\n    api: responses\n    api_key: k\n    default_model: m\n",
            server.uri()
        ),
    )
    .unwrap();
    home
}

async fn lexi_run(home: &std::path::Path, args: &[&str]) -> std::process::Output {
    let mut cmd = lexi(home);
    cmd.args(args);
    run(cmd).await
}

#[tokio::test]
async fn detach_then_status_then_wait() {
    let server = server_with_job().await;
    let home = home_for(&server);
    let h = home.path();

    let o = lexi_run(
        h,
        &[
            "--json",
            "respond",
            "p",
            "--text",
            "go",
            "--background",
            "--detach",
        ],
    )
    .await;
    assert!(o.status.success(), "{}", stderr(&o));
    let v: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["id"], "resp_1");
    assert_eq!(v["state"], "pending");
    assert_eq!(v["provider"], "p");
    let body: Value =
        serde_json::from_slice(&server.received_requests().await.unwrap()[0].body).unwrap();
    assert_eq!(body["background"], true);

    let o = lexi_run(h, &["jobs", "status", "p", "resp_1"]).await;
    assert_eq!(stdout(&o), "resp_1: running (in_progress)\n");

    let o = lexi_run(h, &["jobs", "wait", "p", "resp_1", "--interval", "1"]).await;
    assert!(o.status.success(), "{}", stderr(&o));
    assert_eq!(stdout(&o), "resp_1: succeeded (completed)\nDone!\n");

    let o = lexi_run(h, &["--json", "jobs", "status", "p", "resp_1"]).await;
    let v: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["done"], true);
    assert_eq!(v["text"], "Done!");
}

#[tokio::test]
async fn background_without_detach_waits_for_the_result() {
    let server = server_with_job().await;
    let home = home_for(&server);
    let o = lexi_run(
        home.path(),
        &["respond", "p", "--text", "go", "--background"],
    )
    .await;
    assert!(o.status.success(), "{}", stderr(&o));
    assert_eq!(stdout(&o), "Done!\n");
}

#[tokio::test]
async fn failed_jobs_exit_1_and_detach_needs_background() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/responses/bad"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"id": "bad", "status": "failed"})),
        )
        .mount(&server)
        .await;
    let home = home_for(&server);
    let o = lexi_run(home.path(), &["jobs", "status", "p", "bad"]).await;
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(stdout(&o), "bad: failed (failed)\n");
    let o = lexi_run(home.path(), &["jobs", "wait", "p", "bad"]).await;
    assert_eq!(o.status.code(), Some(1));
    assert!(stderr(&o).contains("background operation failed"));

    let o = lexi_run(home.path(), &["respond", "p", "--text", "x", "--detach"]).await;
    assert!(stderr(&o).contains("--detach needs --background"));
}
