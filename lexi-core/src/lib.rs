//! `lexi-core` — provider-agnostic engine for the Lexi API-definition format.
//!
//! A pure port of the Python `lexi_cli.api_compose` + `lexi_cli.message_ir`
//! engines: schema-driven request-body composition and response extraction,
//! plus the canonical message IR. Behavioral parity with the Python engine is
//! proven by the shared conformance suite under `/conformance`
//! (see `tests/conformance.rs`).
//!
//! Values are `serde_json::Value`; API definitions are plain data.

pub mod compose;
pub mod extract;
pub mod message_ir;
pub mod path;
pub mod template;
pub mod validate;

pub use compose::compose_request_body;
pub use extract::{
    extract_annotations, extract_text, find_text_items, render_annotations, summarize_activity,
};
pub use message_ir::{
    message_from_dict, normalize_prompt_payload, to_prompt_bindings, ContentPart, Message,
};
pub use path::{get_path, set_path, unset_path};
pub use template::render_template;
pub use validate::validate;
