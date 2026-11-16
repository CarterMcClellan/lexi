//! CLI mechanics: `alias`, `history`, `sh`, `help`, `exit`.

use serde_json::json;

use crate::cli::AliasCmd;
use crate::output::{Io, RunOptions, EXIT_FAILURE};

pub(crate) fn alias(io: &mut Io<'_>, cmd: Option<AliasCmd>) -> u8 {
    let mut aliases = crate::aliases::load();
    match cmd.unwrap_or(AliasCmd::List { names: Vec::new() }) {
        AliasCmd::List { names } => {
            let grouped = crate::aliases::grouped(&aliases);
            let shown: Vec<(&String, &Vec<String>)> = grouped
                .iter()
                .filter(|(cmd, names_for)| {
                    names.is_empty()
                        || names.contains(cmd)
                        || names_for.iter().any(|a| names.contains(a))
                })
                .collect();
            let human = if aliases.is_empty() {
                "No aliases defined.".to_string()
            } else {
                shown
                    .iter()
                    .map(|(cmd, a)| format!("{cmd:<12} - {}", a.join(", ")))
                    .collect::<Vec<_>>()
                    .join("\n")
            };
            let json = json!(shown
                .into_iter()
                .collect::<std::collections::BTreeMap<_, _>>());
            io.result(human, json)
        }
        AliasCmd::Add { name, expansion } => {
            let expansion = expansion.join(" ").trim().to_string();
            if crate::shell::is_command_name(&name) {
                return io.fail(format_args!(
                    "`{name}` is a command; an alias can't shadow it"
                ));
            }
            if shlex::split(&expansion).is_none() {
                return io.fail("the expansion has unbalanced quotes");
            }
            aliases.insert(name.clone(), expansion.clone());
            match crate::aliases::save(&aliases) {
                Ok(()) => io.result(
                    format!("Alias saved: {name} -> {expansion}"),
                    json!({ "alias": name, "expansion": expansion }),
                ),
                Err(e) => io.fail(e),
            }
        }
        AliasCmd::Rm { name } => {
            if aliases.remove(&name).is_none() {
                return io.fail(format_args!("alias {name:?} not found"));
            }
            match crate::aliases::save(&aliases) {
                Ok(()) => io.result(format!("Removed alias {name}."), json!({ "removed": name })),
                Err(e) => io.fail(e),
            }
        }
        AliasCmd::Reset => match crate::aliases::reset() {
            Ok(()) => io.result("Aliases reset to defaults.", json!({ "reset": true })),
            Err(e) => io.fail(e),
        },
    }
}

pub(crate) fn history(io: &mut Io<'_>, count: Option<i64>, clear: bool) -> u8 {
    if clear {
        if let Err(e) = crate::history::clear() {
            return io.fail(e);
        }
        if count.is_none() {
            return io.result("History cleared.", json!({ "cleared": true }));
        }
    }
    let entries = crate::history::load();
    let listing = crate::history::listing(&entries, count);
    let json = json!(listing
        .iter()
        .map(|(n, c)| json!({ "n": n, "command": c }))
        .collect::<Vec<_>>());
    let human = if entries.is_empty() {
        "History is empty.".to_string()
    } else {
        listing
            .iter()
            .map(|(n, c)| format!("{n}: {c}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    io.result(human, json)
}

/// Run `command` through the system shell, inheriting the terminal.
pub(crate) fn sh(io: &mut Io<'_>, command: &str, opts: RunOptions) -> u8 {
    if command.is_empty() {
        return io.fail("usage: sh <command>");
    }
    if !opts.allow_shell {
        return io.fail("shell commands are disabled here");
    }
    let _ = io.out.flush();
    let status = if cfg!(windows) {
        std::process::Command::new("cmd")
            .args(["/C", command])
            .status()
    } else {
        std::process::Command::new("sh")
            .args(["-c", command])
            .status()
    };
    match status {
        Ok(s) => s
            .code()
            .map(|c| c.clamp(0, 255) as u8)
            .unwrap_or(EXIT_FAILURE),
        Err(e) => io.fail(format_args!("could not run the shell: {e}")),
    }
}

const SHORTCUTS: &[(&str, &str)] = &[
    ("?", "Shortcut for 'help'."),
    ("!<command>", "Run a shell command (interactive mode)."),
    (
        "#<N>",
        "Re-run history entry N (#-1 is the last; see `history`).",
    ),
    ("↑ / ↓", "Navigate command history (previous/next)."),
];

/// The "general" (CLI mechanics) and "ai" (Lexi functionality) groups.
const GENERAL: &[&str] = &[
    "help", "sh", "exit", "history", "config", "alias", "version",
];
const AI: &[&str] = &[
    "respond",
    "generate",
    "upload",
    "prompts",
    "providers",
    "models",
    "api",
];

fn group_listing(title: &str, names: &[&str]) -> String {
    use clap::CommandFactory;
    let root = crate::Cli::command();
    let mut lines = vec![format!("{title}:")];
    for name in names {
        if let Some(cmd) = root.find_subcommand(name) {
            let about = cmd.get_about().map(|a| a.to_string()).unwrap_or_default();
            let subs: Vec<&str> = cmd
                .get_subcommands()
                .filter(|s| s.get_name() != "help")
                .map(|s| s.get_name())
                .collect();
            let subs = if subs.is_empty() {
                String::new()
            } else {
                format!(" [{}]", subs.join(", "))
            };
            lines.push(format!("  {name:<10} {about}{subs}"));
        }
    }
    lines.join("\n")
}

pub(crate) async fn help(io: &mut Io<'_>, topic: Vec<String>, opts: RunOptions) -> u8 {
    let first = topic.first().map(|s| s.to_lowercase());
    match first.as_deref() {
        Some("general" | "commands") => {
            io.result(group_listing("General commands", GENERAL), json!(GENERAL))
        }
        Some("ai" | "ai-commands" | "lexi" | "lexi-commands") => {
            io.result(group_listing("Lexi commands", AI), json!(AI))
        }
        Some("shortcuts") => {
            let human = std::iter::once("Shortcuts:".to_string())
                .chain(SHORTCUTS.iter().map(|(k, v)| format!("  {k:<12} {v}")))
                .collect::<Vec<_>>()
                .join("\n");
            io.result(
                human,
                json!(SHORTCUTS
                    .iter()
                    .map(|(k, v)| json!({ "shortcut": k, "description": v }))
                    .collect::<Vec<_>>()),
            )
        }
        _ => {
            // Command help: render clap's help for the (alias-resolved) path.
            let path = match crate::shell::expand_aliases(topic) {
                Ok(p) => p,
                Err(e) => return io.fail(e),
            };
            let mut argv = vec!["lexi".to_string()];
            argv.extend(path.iter().cloned());
            argv.push("--help".into());
            let json = std::mem::replace(&mut io.json, false);
            let code = Box::pin(crate::run_parsed(argv, io, opts)).await;
            io.json = json;
            code
        }
    }
}
