//! The `lexi` binary: wires the process's argv and stdio to [`lexi_cli::run`].

use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    let mut out = std::io::stdout();
    let mut err = std::io::stderr();
    let code = lexi_cli::run(std::env::args_os(), &mut out, &mut err).await;
    ExitCode::from(code)
}
