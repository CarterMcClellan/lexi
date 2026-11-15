//! `lexi-core` — provider-agnostic engine for the Lexi API-definition format.
//!
//! A pure port of the Python `lexi_cli.api_compose` + `lexi_cli.message_ir`
//! engines: schema-driven request-body composition and response extraction,
//! plus the canonical message IR. Behavioral parity with the Python engine is
//! proven by the shared conformance suite under `/conformance`
//! (see `tests/conformance.rs`).
//!
//! Values are `serde_json::Value`; API definitions are plain data.

pub mod capabilities;
pub mod compose;
pub mod extract;
pub mod message_ir;
pub mod operations;
pub mod path;
pub mod stream;
pub mod template;
pub mod uploads;
pub mod validate;

pub use capabilities::{resolve_capabilities, supports_input, supports_output, validate_message};
pub use compose::compose_request_body;
pub use extract::{
    extract_annotations, extract_artifacts, extract_text, find_text_items, render_annotations,
    summarize_activity,
};
pub use message_ir::{
    lower_message, message_from_dict, normalize_prompt_payload, to_prompt_bindings, ContentPart,
    Message,
};
pub use operations::{classify_status, is_terminal, poll_path, read_operation};
pub use path::{get_path, set_path, unset_path};
pub use stream::{classify_event, parse_sse_block};
pub use template::render_template;
pub use uploads::{read_upload, upload_path, upload_purpose_field};
pub use validate::validate;
