//! Files-API upload: POST a local file (multipart) and return its handle.
//!
//! The engine describes the upload (`upload_path` / `read_upload`); the host
//! does the multipart HTTP. The returned id can then be referenced as an image
//! part via `--image file:<id>`.

use std::path::Path;

use reqwest::header::CONTENT_TYPE;
use reqwest::multipart::{Form, Part};
use serde_json::Value;

use crate::image::guess_media_type;
use crate::{auth, config, HostError};

/// Upload `file_path` to the API's Files endpoint and return the file id.
pub async fn upload_file_with_effective(
    client: &reqwest::Client,
    effective_cfg: &Value,
    api_key: &str,
    file_path: &Path,
) -> Result<String, HostError> {
    let files_cfg = effective_cfg
        .get("files")
        .cloned()
        .ok_or_else(|| HostError::Config("this API defines no `files` (upload) endpoint".into()))?;

    let base = effective_cfg
        .get("url")
        .and_then(Value::as_str)
        .unwrap_or("");
    let url = format!("{base}{}", lexi_core::upload_path(Some(&files_cfg)));

    let bytes = std::fs::read(file_path)?;
    let filename = file_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "upload".into());
    let mime = guess_media_type(&filename);

    let part = Part::bytes(bytes)
        .file_name(filename)
        .mime_str(&mime)
        .map_err(HostError::Http)?;
    let mut form = Form::new().part("file", part);
    // OpenAI-style uploads require a `purpose`; images use "vision".
    if let Some(field) = lexi_core::upload_purpose_field(Some(&files_cfg)) {
        form = form.text(field, "vision");
    }

    // Multipart sets its own Content-Type (with boundary); drop the JSON one.
    let mut headers = auth::build_headers(effective_cfg, api_key);
    headers.remove(CONTENT_TYPE);

    let resp = client
        .post(&url)
        .headers(headers)
        .multipart(form)
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

    match lexi_core::read_upload(&raw, Some(&files_cfg))
        .get("id")
        .and_then(Value::as_str)
    {
        Some(id) => Ok(id.to_string()),
        None => Err(HostError::Config("upload response had no file id".into())),
    }
}

/// Load config, resolve the provider, and upload a file.
pub async fn upload_file(provider: &str, file_path: &Path) -> Result<String, HostError> {
    let providers = config::load_providers()?;
    let provider_cfg = providers
        .get("providers")
        .and_then(|p| p.get(provider))
        .cloned()
        .ok_or_else(|| HostError::UnknownProvider(provider.to_string()))?;
    let eff = config::effective_config(provider, &provider_cfg)?;
    let api_key = config::resolve_api_key(provider, &provider_cfg)?;
    let client = reqwest::Client::new();
    upload_file_with_effective(&client, &eff, &api_key, file_path).await
}
