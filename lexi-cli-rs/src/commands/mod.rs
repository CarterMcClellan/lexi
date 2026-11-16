//! Command handlers: each turns parsed arguments into `lexi-host` calls and
//! reports through [`Io`]. One module per command group.

mod api;
mod chat;
pub(crate) mod config;
mod jobs;
mod models;
mod prompts;
mod providers;
mod respond;
pub(crate) mod shell_cmds;

use crate::cli::Cmd;
use crate::output::{Io, RunOptions};

pub(crate) async fn dispatch(cmd: Cmd, io: &mut Io<'_>, opts: RunOptions) -> u8 {
    match cmd {
        Cmd::Respond { provider, args } => respond::respond(io, &provider, &args).await,
        Cmd::Generate {
            provider,
            prompt,
            model,
            out,
        } => chat::generate(io, provider, prompt, model, out).await,
        Cmd::Upload { provider, file } => chat::upload(io, provider, file).await,
        Cmd::Jobs { cmd } => jobs::run(io, cmd).await,
        Cmd::Version => chat::version(io),
        Cmd::Help { topic } => shell_cmds::help(io, topic, opts).await,
        Cmd::Alias { cmd } => shell_cmds::alias(io, cmd),
        Cmd::History { count, clear } => shell_cmds::history(io, count, clear),
        Cmd::Sh { command } => shell_cmds::sh(io, &command.join(" "), opts),
        Cmd::Exit => crate::output::EXIT_OK,
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
