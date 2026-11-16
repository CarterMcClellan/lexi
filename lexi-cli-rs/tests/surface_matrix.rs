//! Enforces `docs/surface-matrix.md`: every `lexi-host` operation is classified,
//! and every operation's CLI cell names real `lexi` commands (or a planned
//! consolidation slice), so the CLI can't silently fall behind the runtime.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use clap::CommandFactory;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

/// Names wrapped in backticks within `s`.
fn backticked(s: &str) -> Vec<String> {
    s.split('`')
        .enumerate()
        .filter(|(i, _)| i % 2 == 1)
        .map(|(_, t)| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect()
}

/// The lines of the `## <title>` section.
fn section<'a>(doc: &'a str, title: &str) -> Vec<&'a str> {
    let header = format!("## {title}");
    doc.lines()
        .skip_while(|l| l.trim() != header)
        .skip(1)
        .take_while(|l| !l.starts_with("## "))
        .collect()
}

struct Row {
    operation: String,
    host_fns: Vec<String>,
    cli: String,
}

fn operation_rows(doc: &str) -> Vec<Row> {
    section(doc, "Operations")
        .into_iter()
        .filter(|l| l.starts_with('|') && !l.starts_with("|---"))
        .skip(1) // header row
        .map(|l| {
            let cells: Vec<&str> = l
                .trim()
                .trim_matches('|')
                .split('|')
                .map(str::trim)
                .collect();
            assert_eq!(cells.len(), 5, "matrix row must have 5 cells: {l}");
            Row {
                operation: cells[0].to_string(),
                host_fns: backticked(cells[1]),
                cli: cells[2].to_string(),
            }
        })
        .collect()
}

/// Top-level `pub fn` / `pub async fn` names across `lexi-host/src/*.rs`.
fn host_pub_fns() -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let dir = repo_root().join("lexi-host/src");
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let src = std::fs::read_to_string(&path).unwrap();
        for line in src.lines() {
            let rest = line
                .strip_prefix("pub async fn ")
                .or_else(|| line.strip_prefix("pub fn "));
            if let Some(rest) = rest {
                let name: String = rest
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                names.insert(name);
            }
        }
    }
    names
}

/// Every leaf command path of the `lexi` CLI, e.g. `"models alias set"`.
fn cli_leaf_paths() -> BTreeSet<String> {
    fn walk(cmd: &clap::Command, prefix: &str, out: &mut BTreeSet<String>) {
        let subs: Vec<_> = cmd
            .get_subcommands()
            .filter(|s| s.get_name() != "help")
            .collect();
        if subs.is_empty() {
            out.insert(prefix.to_string());
        }
        for sub in subs {
            let path = if prefix.is_empty() {
                sub.get_name().to_string()
            } else {
                format!("{prefix} {}", sub.get_name())
            };
            walk(sub, &path, out);
        }
    }
    let mut out = BTreeSet::new();
    walk(&lexi_cli::Cli::command(), "", &mut out);
    out
}

fn matrix() -> String {
    std::fs::read_to_string(repo_root().join("docs/surface-matrix.md")).unwrap()
}

#[test]
fn every_host_operation_is_classified_and_every_listed_name_exists() {
    let doc = matrix();
    let rows = operation_rows(&doc);
    let mut listed: BTreeSet<String> = rows.iter().flat_map(|r| r.host_fns.clone()).collect();
    listed.extend(section(&doc, "Internal").into_iter().flat_map(backticked));

    let host = host_pub_fns();
    let unlisted: Vec<_> = host.difference(&listed).collect();
    assert!(
        unlisted.is_empty(),
        "lexi-host pub fns missing from docs/surface-matrix.md (add an Operations row \
         with a CLI command, or list under Internal): {unlisted:?}"
    );
    let stale: Vec<_> = listed.difference(&host).collect();
    assert!(
        stale.is_empty(),
        "docs/surface-matrix.md names functions lexi-host doesn't have: {stale:?}"
    );
}

#[test]
fn every_cli_cell_names_real_commands_or_a_planned_slice() {
    let leaves = cli_leaf_paths();
    for row in operation_rows(&matrix()) {
        if row.cli.starts_with("planned (C") {
            continue;
        }
        let cmds = backticked(&row.cli);
        assert!(
            !cmds.is_empty(),
            "operation {:?} has no CLI command (cell: {:?})",
            row.operation,
            row.cli
        );
        for c in cmds {
            assert!(
                leaves.contains(&c),
                "operation {:?} names unknown CLI command {c:?}",
                row.operation
            );
        }
    }
}

#[test]
fn every_cli_command_is_in_the_matrix() {
    let doc = matrix();
    let mut mentioned: BTreeSet<String> = operation_rows(&doc)
        .iter()
        .flat_map(|r| backticked(&r.cli))
        .collect();
    mentioned.extend(section(&doc, "CLI-only").into_iter().flat_map(backticked));
    let missing: Vec<_> = cli_leaf_paths().difference(&mentioned).cloned().collect();
    assert!(
        missing.is_empty(),
        "lexi commands missing from docs/surface-matrix.md: {missing:?}"
    );
}
