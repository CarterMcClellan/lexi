//! Lexi CLI library — the command tree, parser, dispatcher and output behind the
//! `lexi` binary, over `lexi-host` (which is itself over the pure `lexi-core`
//! engine). It reads and writes the same `~/.lexi-cli/` as the desktop app.
//!
//! [`run`] takes argv plus output writers instead of touching the process's
//! stdio, so in-process hosts (tests, the desktop app's CLI console) can run
//! exactly the commands a terminal user types and capture what they print.

pub mod cli;
mod commands;
pub mod output;

use std::ffi::OsString;

use clap::Parser;

pub use cli::Cli;
pub use output::{Io, Writer, EXIT_FAILURE, EXIT_OK, EXIT_USAGE};

/// Parse `args` (argv, including the program name) and run the command,
/// writing to `out`/`err`. Returns the process exit code: [`EXIT_OK`],
/// [`EXIT_FAILURE`] (the command failed) or [`EXIT_USAGE`] (bad arguments).
pub async fn run<I, T>(args: I, out: Writer<'_>, err: Writer<'_>) -> u8
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(e) => {
            // --help / --version are "errors" to clap but successes to us.
            let code = if e.use_stderr() { EXIT_USAGE } else { EXIT_OK };
            let text = e.render().to_string();
            if e.use_stderr() {
                let _ = err.write_all(text.as_bytes());
                let _ = err.flush();
            } else {
                let _ = out.write_all(text.as_bytes());
                let _ = out.flush();
            }
            return code;
        }
    };
    let mut io = Io::new(out, err, cli.json);
    if let Err(e) = lexi_host::ensure_home() {
        io.warn(format_args!("could not prepare the Lexi home: {e}"));
    }
    commands::dispatch(cli.cmd, &mut io).await
}
