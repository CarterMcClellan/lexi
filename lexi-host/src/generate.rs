//! Artifacts: materialize the typed outputs the engine extracts
//! (`extract_artifacts`) — generated images today — as files on disk,
//! downloading a `url` or decoding inline `b64` (the I/O the engine omits).
//! Any response can carry them: an Images API reply, or a chat reply whose
//! image_generation tool added one. Files never overwrite each other:
//! `<prompt-slug>-<n>.<ext>`.

use std::path::{Path, PathBuf};

use base64::prelude::*;
use serde_json::{json, Value};

use crate::respond::{compose_body, endpoint_url};
use crate::{auth, HostError};

/// A generated artifact written to disk.
#[derive(Debug, Clone)]
pub struct SavedArtifact {
    pub path: PathBuf,
    /// The artifact type (`image`, …).
    pub kind: String,
    pub media_type: String,
    pub revised_prompt: Option<String>,
}

impl SavedArtifact {
    /// `{path, type, media_type, revised_prompt}`.
    pub fn to_value(&self) -> Value {
        json!({
            "path": self.path.display().to_string(),
            "type": self.kind,
            "media_type": self.media_type,
            "revised_prompt": self.revised_prompt,
        })
    }
}

/// A filename stem from the prompt: its first words, lowercased, `-`-joined
/// (`"A watercolor fox!"` → `a-watercolor-fox`); `image` when nothing's left.
fn slug(prompt: &str) -> String {
    let mut out = String::new();
    for word in prompt.split_whitespace() {
        let w: String = word
            .chars()
            .filter(|c| c.is_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect();
        if w.is_empty() {
            continue;
        }
        if !out.is_empty() {
            if out.len() + 1 + w.len() > 40 {
                break;
            }
            out.push('-');
        }
        out.push_str(&w.chars().take(40).collect::<String>());
    }
    if out.is_empty() {
        "image".into()
    } else {
        out
    }
}

/// `dir/<stem>-<n>.<ext>` for the first n ≥ 1 that doesn't exist yet.
fn unique_path(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    (1..)
        .map(|n| dir.join(format!("{stem}-{n}.{ext}")))
        .find(|p| !p.exists())
        .expect("an unused file name")
}

fn media_type_for(ext: &str) -> String {
    match ext {
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        _ => "image/png",
    }
    .to_string()
}

/// Pick a file extension for an artifact: from a `url`'s suffix, else `png`.
fn artifact_ext(artifact: &Value) -> String {
    if let Some(url) = artifact.get("url").and_then(Value::as_str) {
        if let Some(ext) = url.rsplit('.').next() {
            let ext = ext.split(['?', '#']).next().unwrap_or(ext);
            if !ext.is_empty() && ext.len() <= 5 && ext.chars().all(|c| c.is_ascii_alphanumeric()) {
                return ext.to_ascii_lowercase();
            }
        }
    }
    "png".to_string()
}

/// Run an image-generation request over an already-resolved effective config and
/// write each artifact under `out_dir`. Testable against a mock server.
pub async fn generate_images_with_effective(
    client: &reqwest::Client,
    effective_cfg: &Value,
    api_key: &str,
    prompt: &str,
    model: &str,
    out_dir: &Path,
) -> Result<Vec<SavedArtifact>, HostError> {
    let body = compose_body(
        effective_cfg,
        prompt,
        "user",
        &[],
        &json!({ "model": model }),
    )?;
    let raw: Value = crate::respond::within_limit(async {
        let resp = client
            .post(endpoint_url(effective_cfg))
            .headers(auth::build_headers(effective_cfg, api_key))
            .json(&body)
            .send()
            .await?;
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            return Err(HostError::HttpStatus {
                status,
                body: resp.text().await.unwrap_or_default(),
            });
        }
        Ok(resp.json().await?)
    })
    .await?;

    materialize(client, effective_cfg, &raw, out_dir, &slug(prompt)).await
}

/// Write the artifacts in `raw` (per the API's `response.artifacts`) under
/// `out_dir`, named `<stem>-<n>.<ext>`. Artifacts with no `url`/`b64` are skipped.
async fn materialize(
    client: &reqwest::Client,
    effective_cfg: &Value,
    raw: &Value,
    out_dir: &Path,
    stem: &str,
) -> Result<Vec<SavedArtifact>, HostError> {
    let artifacts_cfg = effective_cfg
        .get("response")
        .and_then(|r| r.get("artifacts"));
    let artifacts = lexi_core::extract_artifacts(raw, artifacts_cfg);
    let mut saved = Vec::new();
    for art in &artifacts {
        let bytes: Vec<u8> = if let Some(url) = art.get("url").and_then(Value::as_str) {
            crate::respond::within_limit(async {
                let r = client.get(url).send().await?;
                if !r.status().is_success() {
                    let status = r.status().as_u16();
                    return Err(HostError::HttpStatus {
                        status,
                        body: format!("downloading artifact {url}"),
                    });
                }
                Ok(r.bytes().await?.to_vec())
            })
            .await?
        } else if let Some(b64) = art.get("b64").and_then(Value::as_str) {
            BASE64_STANDARD
                .decode(b64)
                .map_err(|e| HostError::Config(format!("invalid base64 artifact: {e}")))?
        } else {
            continue; // no materializable source
        };
        std::fs::create_dir_all(out_dir)?;
        let ext = artifact_ext(art);
        let path = unique_path(out_dir, stem, &ext);
        std::fs::write(&path, &bytes)?;
        saved.push(SavedArtifact {
            path,
            kind: art
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("file")
                .to_string(),
            media_type: media_type_for(&ext),
            revised_prompt: art
                .get("revised_prompt")
                .and_then(Value::as_str)
                .map(String::from),
        });
    }
    Ok(saved)
}

/// Save the artifacts a finished chat's response carried under `out_dir`
/// (none for a plain text reply).
pub async fn save_artifacts(
    chat: &crate::PreparedChat,
    res: &crate::RespondResult,
    out_dir: &Path,
) -> Result<Vec<SavedArtifact>, HostError> {
    if res.raw.is_null() || chat.effective.pointer("/response/artifacts").is_none() {
        return Ok(Vec::new());
    }
    let client = reqwest::Client::new();
    materialize(
        &client,
        &chat.effective,
        &res.raw,
        out_dir,
        &slug(&chat.prompt_text),
    )
    .await
}

/// Load config, resolve the provider, and generate images into `out_dir`.
pub async fn generate_images_text(
    provider: &str,
    prompt: &str,
    model_override: Option<&str>,
    out_dir: &Path,
) -> Result<Vec<SavedArtifact>, HostError> {
    // The same pipeline as `respond`, asking for image output: the binding
    // that produces images (by the model's tags, or the provider's image API
    // and its default model).
    let req = crate::ChatRequest {
        prompt: json!({ "prompt": prompt, "role": "user" }),
        model: model_override.map(String::from),
        output: Some("image".into()),
        ..Default::default()
    };
    let chat = crate::prepare_chat(provider, &req, crate::ChatMode::Buffered)?;
    let res = crate::run_chat(&chat, |_| {}).await?;
    save_artifacts(&chat, &res, out_dir).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_from_prompts() {
        assert_eq!(slug("A watercolor fox!"), "a-watercolor-fox");
        assert_eq!(slug("  ??  "), "image");
        assert_eq!(slug("Ünïcode Straße"), "ünïcode-straße");
        let long = slug("one two three four five six seven eight nine ten eleven");
        assert!(long.len() <= 40, "{long}");
        assert!(long.starts_with("one-two-three"));
    }

    #[test]
    fn unique_paths_never_reuse_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let first = unique_path(dir.path(), "fox", "png");
        assert!(first.ends_with("fox-1.png"));
        std::fs::write(&first, b"x").unwrap();
        assert!(unique_path(dir.path(), "fox", "png").ends_with("fox-2.png"));
    }
}
