//! Image generation: run a text→image request and materialize the artifacts.
//!
//! The engine extracts typed artifacts (`extract_artifacts`); this module turns
//! each into a file on disk — downloading a `url`, or decoding inline `b64` —
//! the I/O the engine deliberately omits.

use std::path::{Path, PathBuf};

use base64::prelude::*;
use serde_json::{json, Value};

use crate::respond::{compose_body, endpoint_url};
use crate::{auth, config, HostError};

/// A generated artifact written to disk.
#[derive(Debug, Clone)]
pub struct SavedArtifact {
    pub path: PathBuf,
    pub revised_prompt: Option<String>,
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
    let raw: Value = resp.json().await?;

    let artifacts_cfg = effective_cfg
        .get("response")
        .and_then(|r| r.get("artifacts"));
    let artifacts = lexi_core::extract_artifacts(&raw, artifacts_cfg);

    std::fs::create_dir_all(out_dir)?;
    let mut saved = Vec::new();
    for (i, art) in artifacts.iter().enumerate() {
        let bytes: Vec<u8> = if let Some(url) = art.get("url").and_then(Value::as_str) {
            let r = client.get(url).send().await?;
            if !r.status().is_success() {
                let status = r.status().as_u16();
                return Err(HostError::HttpStatus {
                    status,
                    body: format!("downloading artifact {url}"),
                });
            }
            r.bytes().await?.to_vec()
        } else if let Some(b64) = art.get("b64").and_then(Value::as_str) {
            BASE64_STANDARD
                .decode(b64)
                .map_err(|e| HostError::Config(format!("invalid base64 artifact: {e}")))?
        } else {
            continue; // no materializable source
        };

        let path = out_dir.join(format!("image_{i}.{}", artifact_ext(art)));
        std::fs::write(&path, &bytes)?;
        saved.push(SavedArtifact {
            path,
            revised_prompt: art
                .get("revised_prompt")
                .and_then(Value::as_str)
                .map(String::from),
        });
    }
    Ok(saved)
}

/// Load config, resolve the provider, and generate images into `out_dir`.
pub async fn generate_images_text(
    provider: &str,
    prompt: &str,
    model_override: Option<&str>,
    out_dir: &Path,
) -> Result<Vec<SavedArtifact>, HostError> {
    let providers = config::load_providers()?;
    let provider_cfg = providers
        .get("providers")
        .and_then(|p| p.get(provider))
        .cloned()
        .ok_or_else(|| HostError::UnknownProvider(provider.to_string()))?;
    let eff = config::effective_config(provider, &provider_cfg)?;
    let api_key = config::resolve_api_key(provider, &provider_cfg)?;
    let model = model_override
        .map(String::from)
        .or_else(|| {
            eff.get("default_model")
                .and_then(Value::as_str)
                .map(String::from)
        })
        .ok_or_else(|| HostError::Config("no model (pass --model or set default_model)".into()))?;

    let client = reqwest::Client::new();
    generate_images_with_effective(&client, &eff, &api_key, prompt, &model, out_dir).await
}
