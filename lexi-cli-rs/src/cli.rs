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
    /// View CLI settings (config.json).
    Config {
        #[command(subcommand)]
        cmd: ConfigCmd,
    },
    /// Manage saved prompts.
    Prompts {
        #[command(subcommand)]
        cmd: PromptsCmd,
    },
    /// Manage providers.
    Providers {
        #[command(subcommand)]
        cmd: ProvidersCmd,
    },
    /// Manage a provider's models.
    Models {
        #[command(subcommand)]
        cmd: ModelsCmd,
    },
    /// Manage API definitions.
    Api {
        #[command(subcommand)]
        cmd: ApiCmd,
    },
}

#[derive(Debug, Subcommand)]
pub enum ConfigCmd {
    /// Show config values (all, or a single key).
    #[command(visible_alias = "ls")]
    List {
        /// Show only this key.
        key: Option<String>,
        /// Print the raw config file.
        #[arg(short, long)]
        raw: bool,
        /// With a key: print only its value.
        #[arg(short, long)]
        value: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum PromptsCmd {
    /// List prompts (or show one). The active prompt is marked `*`.
    #[command(visible_alias = "ls")]
    List {
        /// Show only this prompt.
        name: Option<String>,
        /// Print the prompt JSON.
        #[arg(short, long)]
        raw: bool,
        /// Include max_tokens / temperature / role.
        #[arg(short, long)]
        detailed: bool,
        /// Render as a table.
        #[arg(short, long)]
        table: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum ProvidersCmd {
    /// List configured providers.
    #[command(visible_alias = "ls")]
    List {
        /// Print the raw providers-config.yaml.
        #[arg(short, long)]
        raw: bool,
        /// Render as a table.
        #[arg(short, long)]
        table: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum ModelsCmd {
    /// List a provider's configured models.
    #[command(visible_alias = "ls")]
    List {
        provider: String,
        /// Print the raw configured-models.yaml.
        #[arg(short, long)]
        raw: bool,
        /// With --raw: also print the raw model-aliases.yaml.
        #[arg(short, long)]
        aliases: bool,
        /// Render as a table.
        #[arg(short, long)]
        table: bool,
        /// Include every metadata field.
        #[arg(short, long)]
        details: bool,
    },
    /// List the models a provider hosts (live call to its models endpoint).
    Hosted {
        provider: String,
        /// Print the models as JSON.
        #[arg(short, long)]
        raw: bool,
        /// Render as a table.
        #[arg(short, long)]
        table: bool,
    },
    /// Show a provider's default model, or set it.
    Default {
        provider: String,
        /// The model to make the default (must be configured).
        model: Option<String>,
    },
    /// Manage a provider's model aliases.
    Alias {
        #[command(subcommand)]
        cmd: ModelAliasCmd,
    },
}

#[derive(Debug, Subcommand)]
pub enum ModelAliasCmd {
    /// List a provider's model aliases.
    #[command(visible_alias = "ls")]
    List {
        provider: String,
        /// Print the raw model-aliases.yaml.
        #[arg(short, long)]
        raw: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum ApiCmd {
    /// List API definitions (shipped and custom).
    #[command(visible_alias = "ls")]
    List {
        /// Print every definition as JSON.
        #[arg(short, long)]
        raw: bool,
    },
    /// Show an API definition.
    Show {
        name: String,
        /// Print the definition as JSON.
        #[arg(short, long)]
        raw: bool,
    },
    /// Validate an API definition (exit 1 if invalid).
    Validate { name: String },
}
