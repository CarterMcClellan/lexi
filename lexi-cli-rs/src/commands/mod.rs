//! Command handlers: each turns parsed arguments into `lexi-host` calls and
//! reports through [`Io`]. One module per command group.

mod api;
mod chat;
pub(crate) mod config;
mod embed;
mod jobs;
mod models;
mod prompts;
mod providers;
mod respond;
pub(crate) mod shell_cmds;

use crate::cli::Cmd;
use crate::output::{Io, RunOptions};

/// Take `--timeout SECS` (or `--timeout=SECS`) out of a free-form argument
/// list: the time limit for this command's provider requests.
fn take_timeout(args: &mut Vec<String>) -> Result<Option<u64>, String> {
    let Some(pos) = args
        .iter()
        .position(|a| a == "--timeout" || a.starts_with("--timeout="))
    else {
        return Ok(None);
    };
    let flag = args.remove(pos);
    let value = match flag.strip_prefix("--timeout=") {
        Some(v) => v.to_string(),
        None if pos < args.len() => args.remove(pos),
        None => return Err("--timeout needs a number of seconds (0 = no limit)".into()),
    };
    value.trim().parse().map(Some).map_err(|_| {
        format!("--timeout is a whole number of seconds (0 = no limit), not {value:?}")
    })
}

/// Run `fut` with `timeout` as its time limit, if one was given.
async fn timed<F: std::future::Future<Output = u8>>(timeout: Option<u64>, fut: F) -> u8 {
    match timeout {
        Some(secs) => lexi_host::with_request_timeout(secs, fut).await,
        None => fut.await,
    }
}

pub(crate) async fn dispatch(cmd: Cmd, io: &mut Io<'_>, opts: RunOptions) -> u8 {
    match cmd {
        Cmd::Respond { provider, mut args } => match take_timeout(&mut args) {
            Ok(t) => timed(t, respond::respond(io, &provider, &args)).await,
            Err(e) => io.fail(e),
        },
        Cmd::Generate {
            provider,
            prompt,
            model,
            out,
            timeout,
        } => timed(timeout, chat::generate(io, provider, prompt, model, out)).await,
        Cmd::Embed { provider, mut args } => match take_timeout(&mut args) {
            Ok(t) => timed(t, embed::embed(io, &provider, &args)).await,
            Err(e) => io.fail(e),
        },
        Cmd::Upload { provider, file } => chat::upload(io, provider, file).await,
        Cmd::Jobs { cmd } => jobs::run(io, cmd).await,
        Cmd::Tags => models::vocabulary(io),
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
