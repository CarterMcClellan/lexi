//! Chat and media commands: `respond`, `generate`, `upload`, `version`.

use serde_json::{json, Value};

use crate::output::{Io, EXIT_FAILURE, EXIT_OK};

#[allow(clippy::too_many_arguments)]
pub(crate) async fn respond(
    io: &mut Io<'_>,
    provider: String,
    prompt: String,
    role: String,
    model: Option<String>,
    stream: bool,
    background: bool,
    images: Vec<String>,
) -> u8 {
    let resolved = match lexi_host::resolve_images(&images) {
        Ok(r) => r,
        Err(e) => return io.fail(e),
    };
    if stream {
        return respond_stream(io, &provider, &prompt, &role, model.as_deref(), &resolved).await;
    }
    let result = if background {
        lexi_host::respond_background_text(&provider, &prompt, &role, model.as_deref()).await
    } else {
        lexi_host::respond_text(&provider, &prompt, &role, model.as_deref(), &resolved).await
    };
    match result {
        Ok(res) => {
            let model = res.raw.get("model").cloned().unwrap_or(Value::Null);
            let json = json!({ "provider": provider, "model": model, "text": res.text });
            io.result(res.text, json)
        }
        Err(e) => io.fail(e),
    }
}

pub(crate) async fn generate(
    io: &mut Io<'_>,
    provider: String,
    prompt: String,
    model: Option<String>,
    out: std::path::PathBuf,
) -> u8 {
    match lexi_host::generate_images_text(&provider, &prompt, model.as_deref(), &out).await {
        Ok(saved) => {
            let human = saved
                .iter()
                .map(|a| a.path.display().to_string())
                .collect::<Vec<_>>()
                .join("\n");
            let artifacts: Vec<Value> = saved
                .iter()
                .map(|a| {
                    json!({
                        "path": a.path.display().to_string(),
                        "revised_prompt": a.revised_prompt,
                    })
                })
                .collect();
            io.result(
                human,
                json!({ "provider": provider, "artifacts": artifacts }),
            )
        }
        Err(e) => io.fail(e),
    }
}

pub(crate) async fn upload(io: &mut Io<'_>, provider: String, file: std::path::PathBuf) -> u8 {
    match lexi_host::upload_file(&provider, &file).await {
        Ok(id) => io.result(&id, json!({ "provider": provider, "file_id": id })),
        Err(e) => io.fail(e),
    }
}

pub(crate) fn version(io: &mut Io<'_>) -> u8 {
    let version = env!("CARGO_PKG_VERSION");
    io.result(
        format!("lexi {version} (rust)"),
        json!({ "version": version, "implementation": "rust" }),
    )
}

/// Streaming respond. Human mode prints deltas as they arrive; machine mode
/// emits NDJSON events with the same shapes as the desktop app's stream:
/// `provider_start`, `delta`, `provider_done` (with `error` on failure).
async fn respond_stream(
    io: &mut Io<'_>,
    provider: &str,
    prompt: &str,
    role: &str,
    model: Option<&str>,
    images: &[lexi_host::ResolvedImage],
) -> u8 {
    let json = io.json;
    if json {
        io.json_line(&json!({ "event": "provider_start", "provider": provider }));
    }
    let result = {
        let out = &mut *io.out;
        lexi_host::respond_stream_text(provider, prompt, role, model, images, |delta| {
            if json {
                let ev = json!({ "event": "delta", "provider": provider, "text": delta });
                let _ = writeln!(out, "{ev}");
            } else {
                let _ = write!(out, "{delta}");
            }
            let _ = out.flush();
        })
        .await
    };
    match result {
        Ok(_) if json => {
            io.json_line(&json!({ "event": "provider_done", "provider": provider }));
            EXIT_OK
        }
        Ok(_) => {
            let _ = writeln!(io.out);
            EXIT_OK
        }
        Err(e) if json => {
            io.json_line(&json!({
                "event": "provider_done",
                "provider": provider,
                "error": e.to_string(),
            }));
            EXIT_FAILURE
        }
        Err(e) => {
            let _ = writeln!(io.out);
            io.fail(e)
        }
    }
}
