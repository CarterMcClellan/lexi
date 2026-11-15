//! Lexi CLI — a thin shell over `lexi-host` (which is itself a thin shell over
//! the pure `lexi-core` engine). This is the Rust CLI the target architecture
//! calls for; it reads the same `~/.lexi-cli/` config as the Python CLI.

use std::process::ExitCode;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "lexi",
    about = "Lexi — multi-provider LLM CLI (Rust host over lexi-core)"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
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
    },
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Respond {
            provider,
            prompt,
            role,
            model,
        } => match lexi_host::respond_text(&provider, &prompt, &role, model.as_deref()).await {
            Ok(res) => {
                println!("{}", res.text);
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("error: {e}");
                ExitCode::FAILURE
            }
        },
    }
}
