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
    /// Send a saved prompt (the active one by default) or --text to a provider.
    ///
    /// respond <provider> [prompt_name] [--text TEXT|-] [--model M] [--role R]
    /// [--stream] [--background] [--image URL|PATH]... [<schema flags>]
    /// [--set path=value]... [--unset path]... [--tool preset[,preset]]...
    /// [--params FILE] [--cite] [--raw|-r] [--verbose|-v] [--no-validate]
    ///
    /// Schema flags (e.g. --temperature, --max-tokens) come from the provider's
    /// API definition: see `lexi api show <api>` (the cli= entries).
    #[command(verbatim_doc_comment)]
    Respond {
        /// Provider name, as in providers-config.yaml.
        provider: String,
        /// Prompt name and options (see above).
        #[arg(
            trailing_var_arg = true,
            allow_hyphen_values = true,
            value_name = "ARGS"
        )]
        args: Vec<String>,
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
    /// Set a config value (the rest of the line is the value).
    Set {
        key: String,
        #[arg(required = true, num_args = 1..)]
        value: Vec<String>,
    },
    /// Remove a config value.
    #[command(visible_aliases = ["del", "delete"])]
    Rm { key: String },
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
    /// Save or update a prompt: `prompts set [name] [text…] [--prompt TEXT]
    /// [-m|--max_tokens N] [-t|--temperature T] [-r|--role ROLE]
    /// [--set path=value] [--unset path]`. With one bare word and no name, it
    /// is the text of the active prompt. Text `\n` becomes a newline.
    #[command(visible_alias = "save")]
    Set {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Remove a saved prompt.
    #[command(visible_aliases = ["del", "delete"])]
    Rm { name: String },
    /// Make a saved prompt the active one (copies it into `$$`).
    #[command(visible_alias = "use")]
    Activate { name: String },
    /// Print an ad-hoc prompt payload (defaults + text/options) without saving.
    #[command(external_subcommand)]
    Text(Vec<String>),
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
    /// Check a provider's API key (live call to its models endpoint).
    Status {
        name: String,
        /// Print {provider, status, source} as JSON.
        #[arg(short, long)]
        raw: bool,
    },
    /// Add a provider.
    Add {
        name: String,
        /// Base URL, e.g. https://api.example.com/v1.
        #[arg(long)]
        url: String,
        /// The model used when none is given.
        #[arg(long = "default_model", visible_alias = "default-model")]
        default_model: String,
        /// API key (stored in the provider's key file, not the config).
        #[arg(long = "api_key", visible_alias = "api-key")]
        api_key: Option<String>,
        /// API definition the provider speaks.
        #[arg(long, default_value = "chat-completions")]
        api: String,
        /// Path of the provider's models endpoint.
        #[arg(
            long = "hosted_models",
            visible_alias = "hosted-models",
            default_value = "/models"
        )]
        hosted_models: String,
    },
    /// Manage a provider's stored API key.
    #[command(name = "api-key")]
    ApiKey {
        #[command(subcommand)]
        cmd: ApiKeyCmd,
    },
    /// Rename a provider.
    Rename { old: String, new: String },
    /// Remove a provider and its models, aliases and key file.
    #[command(visible_aliases = ["del", "delete"])]
    Rm { name: String },
}

#[derive(Debug, Subcommand)]
pub enum ApiKeyCmd {
    /// Store an API key (prompted for, or read from stdin, without --value).
    Set {
        name: String,
        #[arg(short, long)]
        value: Option<String>,
    },
    /// Show the API key in use, masked unless --reveal.
    Show {
        name: String,
        #[arg(short, long)]
        reveal: bool,
    },
    /// Delete the stored key file.
    Remove { name: String },
}

/// Model metadata flags shared by `models add` / `models set`.
#[derive(Debug, clap::Args)]
pub struct ModelMeta {
    /// Display name.
    #[arg(long)]
    pub name: Option<String>,
    #[arg(long)]
    pub description: Option<String>,
    #[arg(long = "max_tokens", visible_alias = "max-tokens")]
    pub max_tokens: Option<u64>,
    #[arg(long = "context_window", visible_alias = "context-window")]
    pub context_window: Option<u64>,
    /// Make this the provider's default model.
    #[arg(long)]
    pub default: bool,
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
    /// Add a model (or update it if it exists).
    Add {
        provider: String,
        model: String,
        #[command(flatten)]
        meta: ModelMeta,
    },
    /// Update a model's metadata (or add it).
    Set {
        provider: String,
        model: String,
        #[command(flatten)]
        meta: ModelMeta,
    },
    /// Remove a model.
    #[command(visible_aliases = ["del", "delete"])]
    Rm { provider: String, model: String },
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
    /// Point an alias at a model.
    Set {
        provider: String,
        alias: String,
        model: String,
    },
    /// Remove an alias.
    #[command(visible_aliases = ["del", "delete"])]
    Rm { provider: String, alias: String },
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
    /// Create a custom API definition (scaffold, or a copy of --from).
    Create {
        name: String,
        /// Copy this definition (shipped or custom).
        #[arg(long)]
        from: Option<String>,
        /// Request path.
        #[arg(long)]
        path: Option<String>,
        /// Response text path (switches to simple text extraction).
        #[arg(long = "text-path")]
        text_path: Option<String>,
    },
    /// Edit a custom API definition.
    Edit {
        name: String,
        #[arg(long)]
        path: Option<String>,
        #[arg(long = "text-path")]
        text_path: Option<String>,
        /// Add response metadata: KEY:PATH. Repeatable.
        #[arg(long = "set-meta", value_name = "KEY:PATH")]
        set_meta: Vec<String>,
    },
    /// Remove a custom API definition.
    #[command(visible_aliases = ["del", "delete"])]
    Rm { name: String },
    /// Preview a definition: validation, CLI flags, tool presets, and a sample
    /// composed request body (always JSON).
    Preview {
        /// A definition by name (or use --file).
        #[arg(required_unless_present = "file", conflicts_with = "file")]
        name: Option<String>,
        /// Preview a YAML file instead of a saved definition.
        #[arg(long)]
        file: Option<PathBuf>,
        #[arg(long, default_value = "Sample prompt")]
        prompt: String,
        #[arg(long, default_value = "sample-model")]
        model: String,
        #[arg(long, default_value = "user")]
        role: String,
    },
    /// Save a YAML file as a custom definition (prints validation errors).
    Save { name: String, file: PathBuf },
}
