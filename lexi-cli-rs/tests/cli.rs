//! The `lexi` binary end to end: real process, temp `LEXI_HOME`, mock provider.

use std::path::Path;

use assert_cmd::Command;
use serde_json::{json, Value};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn lexi(home: &Path) -> Command {
    let mut cmd = Command::cargo_bin("lexi").unwrap();
    cmd.env("LEXI_HOME", home);
    cmd
}

/// A home whose only provider, `mock`, speaks the Messages API at `url`.
fn home_with_mock_provider(url: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let cfg = dir.path().join("providers/providers-config.yaml");
    std::fs::create_dir_all(cfg.parent().unwrap()).unwrap();
    std::fs::write(
        &cfg,
        format!(
            "providers:\n  mock:\n    url: {url}\n    api_key: test-key\n    api: messages\n    default_model: claude-x\n"
        ),
    )
    .unwrap();
    dir
}

/// Run the (blocking) binary off the async test runtime.
async fn run(cmd: Command) -> std::process::Output {
    let mut cmd = cmd;
    tokio::task::spawn_blocking(move || cmd.output().unwrap())
        .await
        .unwrap()
}

fn stdout(o: &std::process::Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

#[test]
fn version_human_and_json() {
    let home = tempfile::tempdir().unwrap();
    let o = lexi(home.path()).arg("version").output().unwrap();
    assert!(o.status.success());
    assert!(stdout(&o).starts_with("lexi "));

    let o = lexi(home.path())
        .args(["--json", "version"])
        .output()
        .unwrap();
    let v: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["implementation"], "rust");
    assert_eq!(v["version"], env!("CARGO_PKG_VERSION"));
}

#[test]
fn help_succeeds_and_bad_usage_exits_2() {
    let home = tempfile::tempdir().unwrap();
    let o = lexi(home.path()).arg("--help").output().unwrap();
    assert!(o.status.success());
    assert!(stdout(&o).contains("respond"));

    let o = lexi(home.path()).arg("no-such-command").output().unwrap();
    assert_eq!(o.status.code(), Some(2));
    assert!(o.stdout.is_empty());
}

#[test]
fn first_run_seeds_the_home_without_overwriting() {
    let home = tempfile::tempdir().unwrap();
    lexi(home.path()).arg("version").assert().success();
    let cfg = home.path().join("providers/providers-config.yaml");
    assert!(cfg.exists());
    assert!(home.path().join("aliases.json").exists());
    assert!(home
        .path()
        .join("providers/openai/configured-models.yaml")
        .exists());

    std::fs::write(&cfg, "providers: {}\n").unwrap();
    lexi(home.path()).arg("version").assert().success();
    assert_eq!(std::fs::read_to_string(&cfg).unwrap(), "providers: {}\n");
}

#[tokio::test]
async fn respond_buffered_human_and_json() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "model": "claude-x",
            "content": [{"type": "text", "text": "Hello from the mock."}]
        })))
        .mount(&server)
        .await;
    let home = home_with_mock_provider(&server.uri());

    let mut cmd = lexi(home.path());
    cmd.args(["respond", "mock", "hi"]);
    let o = run(cmd).await;
    assert!(o.status.success(), "{o:?}");
    assert_eq!(stdout(&o), "Hello from the mock.\n");

    let mut cmd = lexi(home.path());
    cmd.args(["respond", "mock", "hi", "--json"]);
    let o = run(cmd).await;
    let v: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(
        v,
        json!({"provider": "mock", "model": "claude-x", "text": "Hello from the mock."})
    );
}

#[tokio::test]
async fn respond_stream_emits_ndjson_events() {
    let server = MockServer::start().await;
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
    let home = home_with_mock_provider(&server.uri());

    let mut cmd = lexi(home.path());
    cmd.args(["respond", "mock", "hi", "--stream"]);
    let o = run(cmd).await;
    assert!(o.status.success(), "{o:?}");
    assert_eq!(stdout(&o), "Hello world\n");

    let mut cmd = lexi(home.path());
    cmd.args(["--json", "respond", "mock", "hi", "--stream"]);
    let o = run(cmd).await;
    let events: Vec<Value> = stdout(&o)
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(
        events,
        vec![
            json!({"event": "provider_start", "provider": "mock"}),
            json!({"event": "delta", "provider": "mock", "text": "Hello"}),
            json!({"event": "delta", "provider": "mock", "text": " world"}),
            json!({"event": "provider_done", "provider": "mock"}),
        ]
    );
}

#[tokio::test]
async fn provider_errors_exit_1_with_json_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(401).set_body_string("unauthorized"))
        .mount(&server)
        .await;
    let home = home_with_mock_provider(&server.uri());

    let mut cmd = lexi(home.path());
    cmd.args(["--json", "respond", "mock", "hi"]);
    let o = run(cmd).await;
    assert_eq!(o.status.code(), Some(1));
    assert!(o.stdout.is_empty());
    let err: Value = serde_json::from_slice(&o.stderr).unwrap();
    assert!(err["error"].as_str().unwrap().contains("401"), "{err}");

    let mut cmd = lexi(home.path());
    cmd.args(["respond", "nope", "hi"]);
    let o = run(cmd).await;
    assert_eq!(o.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&o.stderr).starts_with("error: unknown provider"));
}
