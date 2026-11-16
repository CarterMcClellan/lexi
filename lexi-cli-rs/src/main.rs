//! The `lexi` binary: wires the process's argv and stdio to [`lexi_cli::run`].

use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    use std::io::IsTerminal;
    let mut out = std::io::stdout();
    let mut err = std::io::stderr();
    let color = err.is_terminal() && std::env::var_os("NO_COLOR").is_none();
    let opts = lexi_cli::RunOptions {
        color,
        allow_shell: true,
    };
    let code = lexi_cli::run_with(std::env::args_os(), &mut out, &mut err, opts).await;
    ExitCode::from(code)
}
