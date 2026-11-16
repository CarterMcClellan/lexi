//! C6: the interactive shell (driven through piped stdin), command aliases,
//! history, help topics, and `sh`.

mod common;

use common::{lexi, lexi_out, seeded_home, stderr, stdout};

/// Run a shell session: `lines` piped to `lexi` with no command.
fn session(home: &std::path::Path, lines: &[&str]) -> std::process::Output {
    let script = lines.iter().map(|l| format!("{l}\n")).collect::<String>();
    lexi(home).write_stdin(script).output().unwrap()
}

#[test]
fn shell_runs_lines_records_history_and_replays_entries() {
    let home = seeded_home();
    let o = session(
        home.path(),
        &["version", "config set theme x", "#1", "#-1", "history"],
    );
    assert!(o.status.success(), "{}", stderr(&o));
    let out = stdout(&o);
    assert_eq!(out.matches("lexi 0.1.0 (rust)").count(), 3, "{out}");
    assert!(
        out.ends_with("1: version\n2: config set theme x\n3: version\n4: version\n5: history\n"),
        "{out}"
    );
    // The replayed command is echoed (to stderr) before it runs.
    assert!(stderr(&o).contains("version"));

    // History persists across sessions and is shared with `lexi history`.
    assert_eq!(
        stdout(&lexi_out(home.path(), &["history", "-n", "2"])),
        "4: version\n5: history\n"
    );
    assert_eq!(
        stdout(&lexi_out(home.path(), &["history", "-n", "-1"])),
        "1: version\n"
    );
    assert_eq!(
        stdout(&lexi_out(home.path(), &["history", "--clear"])),
        "History cleared.\n"
    );
    assert_eq!(
        stdout(&lexi_out(home.path(), &["history"])),
        "History is empty.\n"
    );
}

#[test]
fn shell_keeps_going_after_errors_and_stops_at_exit() {
    let home = seeded_home();
    let o = session(
        home.path(),
        &[
            "no-such-command",
            "config list \"unbalanced",
            "#99",
            "exit",
            "version",
        ],
    );
    assert!(o.status.success());
    let err = stderr(&o);
    assert!(err.contains("unrecognized subcommand"), "{err}");
    assert!(err.contains("could not parse command"), "{err}");
    assert!(err.contains("out of range"), "{err}");
    assert!(
        !stdout(&o).contains("lexi 0.1.0"),
        "nothing runs after exit"
    );

    // The shipped `q` alias also exits; so does EOF.
    let o = session(home.path(), &["q", "version"]);
    assert!(!stdout(&o).contains("lexi 0.1.0"));
    let o = session(home.path(), &["version"]);
    assert!(o.status.success() && stdout(&o).contains("lexi 0.1.0"));
}

#[test]
fn aliases_expand_in_the_shell_and_one_off_commands() {
    let home = seeded_home();
    let h = home.path();
    assert!(stdout(&lexi_out(h, &["alias", "list"])).contains("providers    - prov\n"));
    assert_eq!(
        stdout(&lexi_out(h, &["alias", "add", "pl", "providers", "list"])),
        "Alias saved: pl -> providers list\n"
    );
    // One-off: `lexi prov list` and `lexi pl -r` (extra args follow the expansion).
    assert!(stdout(&lexi_out(h, &["prov", "list"])).contains("anthropic: url="));
    assert!(stdout(&lexi_out(h, &["--json", "pl"])).starts_with('['));
    assert!(stdout(&lexi_out(h, &["pl", "-r"])).contains("\nproviders:\n"));
    // In the shell.
    assert!(stdout(&session(h, &["pl"])).contains("anthropic: url="));

    // Aliases never shadow commands; the file keeps the grouped format.
    let o = lexi_out(h, &["alias", "add", "models", "providers"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(stderr(&o).contains("is a command"));
    let file: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(h.join("aliases.json")).unwrap()).unwrap();
    assert_eq!(file["providers list"], serde_json::json!(["pl"]));

    assert_eq!(
        stdout(&lexi_out(h, &["alias", "rm", "pl"])),
        "Removed alias pl.\n"
    );
    assert_eq!(lexi_out(h, &["alias", "rm", "pl"]).status.code(), Some(1));
    assert_eq!(
        stdout(&lexi_out(h, &["alias", "reset"])),
        "Aliases reset to defaults.\n"
    );
    assert!(stdout(&lexi_out(h, &["alias"])).contains("exit         - e, ex, q\n"));
}

#[test]
fn help_topics_and_command_help() {
    let home = seeded_home();
    let h = home.path();
    let root = stdout(&lexi_out(h, &["help"]));
    assert!(root.contains("Usage: lexi") && root.contains("interactive shell"));
    let general = stdout(&lexi_out(h, &["help", "general"]));
    assert!(general.starts_with("General commands:\n") && general.contains("  history"));
    assert!(stdout(&lexi_out(h, &["help", "ai"])).contains("  respond"));
    assert!(stdout(&lexi_out(h, &["help", "shortcuts"])).contains("#<N>"));
    assert!(stdout(&lexi_out(h, &["help", "models", "alias"])).contains("Usage: lexi models alias"));
    // Through an alias, and via `?` in the shell.
    assert!(stdout(&lexi_out(h, &["help", "prov"])).contains("Usage: lexi providers"));
    assert!(stdout(&session(h, &["? shortcuts"])).contains("Shortcuts:"));
}

#[test]
fn sh_runs_through_the_system_shell() {
    let home = seeded_home();
    let o = lexi_out(home.path(), &["sh", "echo", "from-sh"]);
    assert!(o.status.success());
    assert_eq!(stdout(&o).trim(), "from-sh");
    let o = session(home.path(), &["!echo bang"]);
    assert_eq!(stdout(&o).trim(), "bang");
    assert_eq!(
        lexi_out(home.path(), &["sh", "exit 3"]).status.code(),
        Some(3)
    );
}
