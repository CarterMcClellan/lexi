//! Command handlers: each turns parsed arguments into `lexi-host` calls and
//! reports through [`Io`]. One module per command group.

mod api;
mod chat;
mod config;
mod models;
mod prompts;
mod providers;
mod respond;

use crate::cli::Cmd;
use crate::output::Io;

pub(crate) async fn dispatch(cmd: Cmd, io: &mut Io<'_>) -> u8 {
    match cmd {
        Cmd::Respond { provider, args } => respond::respond(io, &provider, &args).await,
        Cmd::Generate {
            provider,
            prompt,
            model,
            out,
        } => chat::generate(io, provider, prompt, model, out).await,
        Cmd::Upload { provider, file } => chat::upload(io, provider, file).await,
        Cmd::Version => chat::version(io),
        Cmd::Config { cmd } => config::run(io, cmd),
        Cmd::Prompts { cmd } => prompts::run(io, cmd),
        Cmd::Providers { cmd } => providers::run(io, cmd).await,
        Cmd::Models { cmd } => models::run(io, cmd).await,
        Cmd::Api { cmd } => api::run(io, cmd),
    }
}

/// Read a file under the Lexi home for `--raw` display.
pub(crate) fn read_home_file(rel: &str) -> std::io::Result<String> {
    std::fs::read_to_string(lexi_host::config::lexi_home().join(rel))
}
