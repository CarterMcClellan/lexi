//! `lexi-host` — the Lexi runtime (I/O layer) over the pure `lexi-core` engine.
//!
//! `lexi-core` makes every pure decision (compose the request body, extract the
//! response); this crate adds the side effects it deliberately omits: loading
//! providers/keys/config, building auth headers, and HTTP transport. It is the
//! shared runtime beneath the CLI and (later) the Tauri GUI.

pub mod auth;
pub mod config;
pub mod respond;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum HostError {
    #[error("unknown provider {0:?}")]
    UnknownProvider(String),
    #[error("provider {0:?} has no `api` reference")]
    MissingApi(String),
    #[error("unknown API definition {0:?}")]
    UnknownApi(String),
    #[error("missing API key for provider {0:?}")]
    MissingApiKey(String),
    #[error("config error: {0}")]
    Config(String),
    #[error("request composition failed: {0}")]
    Compose(String),
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("provider returned HTTP {status}: {body}")]
    HttpStatus { status: u16, body: String },
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("yaml error: {0}")]
    Yaml(#[from] serde_yaml::Error),
}

pub use respond::{
    respond_stream_text, respond_stream_with_effective, respond_text, respond_with_effective,
    RespondResult,
};
