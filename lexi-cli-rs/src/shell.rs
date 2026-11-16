//! The interactive shell (`lexi` with no command) and the line executor it
//! shares with in-process hosts (the desktop app's CLI console).
//!
//! A line is shell-split; `#N` re-runs history entry N (`#-1` the last); `!cmd`
//! runs a shell command; a leading alias expands; `exit`/`quit` leave; anything
//! else runs as a `lexi` command. Each executed line is recorded in history.

use std::ffi::OsString;

use clap::CommandFactory;

use crate::output::{Io, RunOptions, EXIT_OK};

/// Words that leave the shell (when given alone).
const EXIT_WORDS: &[&str] = &["exit", "quit", "e", "q"];

/// The result of one line: its exit code, and whether the shell should exit.
pub struct LineOutcome {
    pub code: u8,
    pub exit: bool,
}

/// Whether `name` is a `lexi` command (or one of its built-in aliases).
pub(crate) fn is_command_name(name: &str) -> bool {
    crate::Cli::command()
        .get_subcommands()
        .any(|c| c.get_name() == name || c.get_all_aliases().any(|a| a == name))
}

/// Expand a leading alias in argv-style `tokens` (after any leading global
/// flags, e.g. `--json`).
pub(crate) fn expand_aliases(tokens: Vec<String>) -> Result<Vec<String>, String> {
    let split = tokens
        .iter()
        .position(|t| !t.starts_with('-'))
        .unwrap_or(tokens.len());
    let (flags, rest) = tokens.split_at(split);
    let aliases = crate::aliases::load();
    let mut out = flags.to_vec();
    out.extend(crate::aliases::expand(
        rest.to_vec(),
        &aliases,
        is_command_name,
    )?);
    Ok(out)
}

/// Run one shell line, writing through `io`. Records it in history.
pub async fn execute_line(line: &str, io: &mut Io<'_>, opts: RunOptions) -> LineOutcome {
    let mut line = line.trim().to_string();
    if line.is_empty() {
        return LineOutcome {
            code: EXIT_OK,
            exit: false,
        };
    }
    // `#N`: replay a history entry (which is then recorded like any line).
    if let Some(reference) = line.strip_prefix('#') {
        let reference = reference.trim();
        let index = match reference {
            "-" => Ok(-1),
            "" => Err("usage: #<number> (e.g. #3, or #-1 for the last command)".to_string()),
            r => r
                .parse::<i64>()
                .map_err(|_| "history reference must be an integer (e.g. #3)".to_string()),
        };
        match index.and_then(|i| crate::history::entry(&crate::history::load(), i)) {
            Ok(target) => {
                let _ = writeln!(io.err, "{}", crate::output::dim(&target, io.color));
                line = target;
            }
            Err(e) => {
                return LineOutcome {
                    code: io.fail(e),
                    exit: false,
                }
            }
        }
    }
    if let Err(e) = crate::history::append(&line) {
        io.warn(format_args!("could not record history: {e}"));
    }
    if let Some(cmd) = line.strip_prefix('!') {
        let code = crate::commands::shell_cmds::sh(io, cmd.trim(), opts);
        return LineOutcome { code, exit: false };
    }
    let tokens = match shlex::split(&line) {
        Some(t) => t,
        None => {
            let code = io.fail(
                "could not parse command (unbalanced quotes?); for multi-line prompts, \
                 escape newlines as \\n",
            );
            return LineOutcome { code, exit: false };
        }
    };
    let tokens = match tokens.split_first() {
        Some((first, rest)) if first == "?" => std::iter::once("help".to_string())
            .chain(rest.iter().cloned())
            .collect(),
        _ => tokens,
    };
    let tokens = match expand_aliases(tokens) {
        Ok(t) => t,
        Err(e) => {
            return LineOutcome {
                code: io.fail(e),
                exit: false,
            }
        }
    };
    if tokens.len() == 1 && EXIT_WORDS.contains(&tokens[0].to_lowercase().as_str()) {
        return LineOutcome {
            code: EXIT_OK,
            exit: true,
        };
    }
    let argv =
        std::iter::once(OsString::from("lexi")).chain(tokens.into_iter().map(OsString::from));
    let code = crate::run_parsed(
        argv.map(|a| a.to_string_lossy().into_owned()).collect(),
        io,
        opts,
    )
    .await;
    LineOutcome { code, exit: false }
}

/// The interactive loop: read lines until EOF (Ctrl-D) or an exit word.
/// Ctrl-C abandons the current line.
pub(crate) async fn repl(io: &mut Io<'_>, opts: RunOptions) -> u8 {
    use rustyline::error::ReadlineError;

    let cfg = lexi_host::admin::settings().unwrap_or_default();
    let get = |k: &str, d: &str| {
        cfg.get(k)
            .map(|v| crate::output::scalar(Some(v)))
            .unwrap_or_else(|| d.to_string())
    };
    let prompt = format!("{}{} ", get("prompt", "cmd"), get("prompt_delimiter", ">"));
    let mode = if get("edit_mode", "vi").eq_ignore_ascii_case("emacs") {
        rustyline::EditMode::Emacs
    } else {
        rustyline::EditMode::Vi
    };
    let config = rustyline::Config::builder()
        .edit_mode(mode)
        .auto_add_history(false)
        .build();
    let mut editor = match rustyline::DefaultEditor::with_config(config) {
        Ok(e) => e,
        Err(e) => return io.fail(format_args!("could not start the shell: {e}")),
    };
    let reload = |editor: &mut rustyline::DefaultEditor| {
        let _ = editor.clear_history();
        for entry in crate::history::load() {
            let _ = editor.add_history_entry(entry);
        }
    };
    reload(&mut editor);

    loop {
        let line = match editor.readline(&prompt) {
            Ok(line) => line,
            Err(ReadlineError::Interrupted) => continue,
            Err(ReadlineError::Eof) => {
                // End the prompt's line on a terminal (not in piped scripts).
                if std::io::IsTerminal::is_terminal(&std::io::stdin()) {
                    let _ = writeln!(io.out);
                }
                return EXIT_OK;
            }
            Err(e) => return io.fail(e),
        };
        let outcome = execute_line(&line, io, opts).await;
        // Re-sync the editor with the file (picks up the new entry, `#N`
        // replays, and `history --clear`).
        reload(&mut editor);
        if outcome.exit {
            return EXIT_OK;
        }
        let _ = outcome.code; // a failed command doesn't end the shell
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// In-process hosts (the desktop console) run lines from `Send` futures.
    #[test]
    fn execute_line_is_send() {
        fn assert_send<T: Send>(_: &T) {}
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let mut io = Io::new(&mut out, &mut err, false);
        let fut = execute_line("version", &mut io, RunOptions::default());
        assert_send(&fut);
    }
}
