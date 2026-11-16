//! Media and misc commands: `generate`, `upload`, `version`.

use serde_json::{json, Value};

use crate::output::Io;

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
