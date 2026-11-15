//! Resolve a CLI image spec (`--image <url|path>`) into a canonical content part.
//!
//! A `http(s)://` spec becomes a URL image part; anything else is treated as a
//! local file — read, base64-encoded, and typed by extension. This is the input
//! counterpart to artifact save: the host handles the file I/O, the engine's
//! `content_mapping` lowers the part to each provider's wire shape.

use base64::prelude::*;
use serde_json::{json, Value};

use crate::HostError;

/// A resolved image source, ready to become a message content part.
#[derive(Debug, Clone)]
pub enum ResolvedImage {
    Url(String),
    Base64 { data: String, media_type: String },
    FileId(String),
}

impl ResolvedImage {
    /// The canonical `{type: image, ...}` content-part value.
    pub fn part_value(&self) -> Value {
        match self {
            ResolvedImage::Url(url) => json!({ "type": "image", "url": url }),
            ResolvedImage::Base64 { data, media_type } => {
                json!({ "type": "image", "data": data, "media_type": media_type })
            }
            ResolvedImage::FileId(id) => json!({ "type": "image", "file_id": id }),
        }
    }
}

pub(crate) fn guess_media_type(path: &str) -> String {
    let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        _ => "image/png",
    }
    .to_string()
}

/// Resolve one `--image` spec: a URL is used as-is; `file:<id>` references a
/// previously uploaded file; anything else is a local path (read + base64).
pub fn resolve_image(spec: &str) -> Result<ResolvedImage, HostError> {
    if spec.starts_with("http://") || spec.starts_with("https://") {
        Ok(ResolvedImage::Url(spec.to_string()))
    } else if let Some(id) = spec.strip_prefix("file:") {
        Ok(ResolvedImage::FileId(id.to_string()))
    } else {
        let bytes = std::fs::read(spec)?;
        Ok(ResolvedImage::Base64 {
            data: BASE64_STANDARD.encode(bytes),
            media_type: guess_media_type(spec),
        })
    }
}

/// Resolve several specs in order.
pub fn resolve_images(specs: &[String]) -> Result<Vec<ResolvedImage>, HostError> {
    specs.iter().map(|s| resolve_image(s)).collect()
}
