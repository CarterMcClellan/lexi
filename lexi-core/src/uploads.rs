//! Files-API uploads: interpret an API def's `files` block.
//!
//! Mirrors `lexi_cli.uploads`. The multipart upload itself is host-side; this
//! module reads the upload path and extracts the file handle from the upload
//! response, so it can be used as an image part's `file_id` source. Pure: no
//! I/O. See `docs/spec/uploads-v0.md`.

use serde_json::{Map, Value};

use crate::path::get_path;

/// Return the Files-API upload path (e.g. `/files`), or `""` if absent.
pub fn upload_path(files_cfg: Option<&Value>) -> String {
    files_cfg
        .and_then(|c| c.get("upload"))
        .and_then(|u| u.get("path"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

/// Return the multipart field name carrying the upload purpose, if any.
pub fn upload_purpose_field(files_cfg: Option<&Value>) -> Option<String> {
    files_cfg
        .and_then(|c| c.get("upload"))
        .and_then(|u| u.get("purpose_field"))
        .and_then(Value::as_str)
        .map(String::from)
}

/// Read the file handle `{ "id": <file id or null> }` from an upload response.
pub fn read_upload(upload_response: &Value, files_cfg: Option<&Value>) -> Value {
    let mut m = Map::new();
    let id = match files_cfg {
        Some(cfg) => {
            let id_path = cfg.get("id_path").and_then(Value::as_str).unwrap_or("id");
            get_path(upload_response, id_path)
                .cloned()
                .unwrap_or(Value::Null)
        }
        None => Value::Null,
    };
    m.insert("id".into(), id);
    Value::Object(m)
}
