//! `docs/cli-reference.md` is generated from the clap command tree; this test
//! fails when it is stale. Regenerate with:
//!
//!     LEXI_UPDATE_DOCS=1 cargo test -p lexi-cli-rs --test cli_reference

use std::fmt::Write;
use std::path::Path;

use clap::CommandFactory;

fn about(cmd: &clap::Command) -> String {
    cmd.get_long_about()
        .or_else(|| cmd.get_about())
        .map(|a| a.to_string())
        .unwrap_or_default()
}

fn visible_subs(cmd: &clap::Command) -> Vec<&clap::Command> {
    cmd.get_subcommands()
        .filter(|s| !s.is_hide_set() && s.get_name() != "help")
        .collect()
}

/// One section per leaf command: heading, aliases, description, usage block.
fn leaf(out: &mut String, path: &str, cmd: &clap::Command) {
    let mut c = cmd.clone().bin_name(format!("lexi {path}"));
    let usage = c.render_usage().to_string();
    let usage = usage.trim_start_matches("Usage: ").trim();
    writeln!(out, "#### `{path}`\n").unwrap();
    let aliases: Vec<&str> = cmd.get_visible_aliases().collect();
    if !aliases.is_empty() {
        writeln!(
            out,
            "Aliases: {}\n",
            aliases
                .iter()
                .map(|a| format!("`{a}`"))
                .collect::<Vec<_>>()
                .join(", ")
        )
        .unwrap();
    }
    let text = about(cmd);
    if !text.is_empty() {
        writeln!(out, "{text}\n").unwrap();
    }
    writeln!(out, "```\n{usage}\n```\n").unwrap();
    let args: Vec<_> = cmd
        .get_arguments()
        .filter(|a| !a.is_hide_set() && a.get_id() != "help" && a.get_id() != "version")
        .collect();
    if !args.is_empty() {
        for a in args {
            let name = match (a.get_short(), a.get_long()) {
                (Some(s), Some(l)) => format!("-{s}, --{l}"),
                (None, Some(l)) => format!("--{l}"),
                (Some(s), None) => format!("-{s}"),
                (None, None) => format!("<{}>", a.get_id().as_str().to_uppercase()),
            };
            let help = a.get_help().map(|h| h.to_string()).unwrap_or_default();
            writeln!(
                out,
                "- `{name}`{}{help}",
                if help.is_empty() { "" } else { " — " }
            )
            .unwrap();
        }
        writeln!(out).unwrap();
    }
}

fn walk(out: &mut String, path: &str, cmd: &clap::Command) {
    let subs = visible_subs(cmd);
    if subs.is_empty() {
        leaf(out, path, cmd);
        return;
    }
    for s in subs {
        walk(out, &format!("{path} {}", s.get_name()), s);
    }
}

fn render() -> String {
    let root = lexi_cli::Cli::command();
    let mut out = String::from(
        "# CLI Reference\n\n\
         Complete command reference for `lexi`, the Rust CLI.\n\
         This file is generated from the command definitions — regenerate with\n\
         `LEXI_UPDATE_DOCS=1 cargo test -p lexi-cli-rs --test cli_reference`.\n\n\
         Run `lexi` with no command for the interactive shell. Every command accepts\n\
         the global `--json` flag (machine-readable output; see\n\
         [spec/cli-machine-v0.md](spec/cli-machine-v0.md)).\n\n---\n\n",
    );
    for top in visible_subs(&root) {
        writeln!(out, "## `{}`\n", top.get_name()).unwrap();
        let text = top.get_about().map(|a| a.to_string()).unwrap_or_default();
        if !visible_subs(top).is_empty() && !text.is_empty() {
            writeln!(out, "{text}\n").unwrap();
        }
        walk(&mut out, top.get_name(), top);
    }
    out.push_str(
        "## Shortcuts (interactive shell)\n\n\
         | Shortcut | Description |\n|---|---|\n\
         | `?` | Shortcut for `help`. |\n\
         | `!<command>` | Run a shell command. |\n\
         | `#<N>` | Re-run history entry N (`#-1` is the last). |\n\
         | `↑ / ↓` | Navigate command history. |\n",
    );
    out
}

#[test]
fn cli_reference_is_current() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs/cli-reference.md");
    let expected = render();
    if std::env::var_os("LEXI_UPDATE_DOCS").is_some() {
        std::fs::write(&path, &expected).unwrap();
        return;
    }
    let actual = std::fs::read_to_string(&path)
        .unwrap_or_default()
        .replace("\r\n", "\n");
    assert!(
        actual == expected,
        "docs/cli-reference.md is stale; regenerate with \
         `LEXI_UPDATE_DOCS=1 cargo test -p lexi-cli-rs --test cli_reference`"
    );
}
