//! The `lexi` command tree (clap).
//!
//! Grammar follows the Python CLI (`docs/cli-reference.md`): plural nouns with
//! verb subcommands (`prompts list`, `models alias set`, …). Commands are added
//! slice by slice as they become fully functional — see
//! `docs/cli-consolidation-plan.md` and `docs/surface-matrix.md`.

use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "lexi",
    version,
    about = "Lexi — work with multiple LLM providers side by side",
    propagate_version = true
)]
pub struct Cli {
    /// Machine-readable output: one JSON value per result (NDJSON events when
    /// streaming); errors as {"error": ...} on stderr.
    #[arg(long, global = true)]
    pub json: bool,

    #[command(subcommand)]
    pub cmd: Cmd,
}

#[derive(Debug, Subcommand)]
pub enum Cmd {
    /// Send a text prompt to a provider and print the response.
    Respond {
        /// Provider name, as in providers-config.yaml (e.g. anthropic, openai).
        provider: String,
        /// The prompt text.
        prompt: String,
        /// Message role.
        #[arg(long, default_value = "user")]
        role: String,
        /// Override the provider's default model.
        #[arg(long)]
        model: Option<String>,
        /// Stream the response token-by-token (SSE).
        #[arg(long, conflicts_with = "background")]
        stream: bool,
        /// Submit as a background operation and poll until it completes.
        #[arg(long)]
        background: bool,
        /// Attach an image (URL or local file path). Repeatable.
        #[arg(long = "image")]
        images: Vec<String>,
    },
    /// Generate image(s) from a prompt and save them to disk.
    Generate {
        /// Provider name (must reference an image-generations API).
        provider: String,
        /// The image prompt.
        prompt: String,
        /// Override the provider's default model.
        #[arg(long)]
        model: Option<String>,
        /// Directory to write images into.
        #[arg(long, default_value = ".")]
        out: PathBuf,
    },
    /// Upload a file to a provider's Files API; prints the file id.
    Upload {
        /// Provider name.
        provider: String,
        /// Path to the file to upload.
        file: PathBuf,
    },
    /// Show the CLI version.
    Version,
}
