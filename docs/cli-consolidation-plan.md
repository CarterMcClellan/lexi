# Lexi CLI Consolidation — Plan

**Status:** Draft · 2026-11-15 · follows the completed host phase ([host-plan.md](host-plan.md)).
**Goal:** one CLI. The Rust `lexi` (`lexi-cli-rs`, over `lexi-host`) becomes the
complete, canonical command surface; the Python CLI application (`lexi-cli/`) is
retired. The `lexi` **wheel** (`lexi-python/`) stays — it is the Python *library*,
not an application.

---

## 1. Why there are two CLIs today

The Python CLI was the original product. The Rust CLI began in H2 as a test
harness for `lexi-host` and grew a subcommand per wired runtime feature. H1b/H1c
moved the Python CLI onto the Rust **engine** (via the wheel), but the wheel wraps
only `lexi-core`, not `lexi-host` — so the Python CLI still has its own HTTP layer
(`urllib` in `cli.py`). Result: each CLI is missing what the other has, and there
are two I/O stacks.

| | Python CLI (`lexi-cli`) | Rust CLI (`lexi-cli-rs`) |
|---|---|---|
| Engine | `lexi-core` via wheel | `lexi-core` |
| HTTP / SSE / keys | own (`cli.py`, `urllib`) | `lexi-host` |
| Shell, history, command aliases, help | ✅ | ❌ |
| prompts / providers / models / config / api admin | ✅ | ❌ |
| `respond` definition flags (`x-cli`), tool presets, `--raw`, named prompts | ✅ | ❌ |
| Image input, image gen, upload, background ops | ❌ | ✅ |
| Encrypted API keys (Fernet) | ✅ | ❌ |
| First-run seeding of `~/.lexi-cli/` | ✅ | ❌ |

The desktop app runs on `lexi-host`, so the Rust CLI is the one that matches the
GUI — and the architecture invariant (*nothing is GUI-only; the CLI is the
complete surface*) can only hold if the CLI sits on the same runtime.

## 2. What `lexi-host` already provides

The desktop admin tabs forced most of the backend into `lexi-host` already, so
most slices are **clap wiring + output formatting**, not new logic.

| Area | Existing `lexi-host` functions | Missing |
|---|---|---|
| Config | `load_config`, `set_config` | `delete_config` (`config rm`) |
| Prompts | `load_prompts`, `save_prompt`, `delete_prompt`, `set_active_prompt` | — |
| Providers | `provider_names`, `list_providers`, `load_providers` | add / rename / rm, api-key set/show/remove, status |
| Models | `load_models`, `default_model`/`set_default_model`, `model_aliases`/`set_model_alias`/`delete_model_alias`, `upsert_model`, `delete_model`, `fetch_hosted_models` | — |
| API defs | `list_api_defs`, `load_api_raw`, `create_api_def`, `save_api_raw`, `validate_api_def`, `preview_api_def`, `delete_api_def` | `api edit` (`--set-param` / `--set-meta`) |
| Respond | `respond_*`, `respond_stream_*`, `respond_background_*`, images | named/active prompt, `x-cli` flags, tool presets, `--set`/`--unset`, `--raw` |
| Keys | `resolve_api_key` (env / plain file) | Fernet-encrypted key files |
| Bootstrap | — | first-run seeding (providers, APIs, default aliases) |
| Command aliases / history | — | `aliases.json`, `history` (CLI-local; may live in `lexi-cli-rs`) |

## 3. Surface principle (decided 2026-11-15)

Core functionality is available on **every surface**, all built on the same library:

| Surface | Audience | Built on |
|---|---|---|
| Library (`lexi-core` + `lexi-host` crates) | embedders | — |
| Programmatic API = **language bindings** (Python wheel today; later C ABI / uniffi) | programs | the library |
| CLI (`lexi`) | advanced users, scripting, automation | the library |
| GUI (`lexi-desktop`) | ease of use | the library |

- A network API (HTTP/JSON via `lexi serve`) is **not** part of the API surface for
  now — deferred (§7).
- The GUI does **not** have to drive the CLI. Both call `lexi-host` directly.
- The GUI may compose several operations with extra logic in between (workflows);
  no 1:1 GUI↔CLI mapping is required for those. **Every underlying operation**,
  though, must be reachable from the CLI and the programmatic API.
- **Divergence guard (replaces "GUI layered on CLI"):** user-facing operations live
  in `lexi-host` as the single implementation; `docs/surface-matrix.md` lists each
  one with its CLI command, API binding and GUI entry point. A CI test enforces the
  **CLI** column now (a new host operation can't merge without a command); the
  **API** column is tracked but only enforced once the wheel wraps `lexi-host`
  (backlog, §7).
- The GUI gets a **CLI console tab** (C9) so GUI users can run `lexi` commands directly.

## 4. Decisions (proposed defaults — confirm or override)

1. **Keep the Python CLI's command grammar.** Same nouns/verbs/flags
   (`prompts list`, `models alias set`, `providers api-key show --reveal`, …), so
   existing muscle memory, aliases in `aliases.json`, and `docs/cli-reference.md`
   carry over. Rust-only commands (`generate`, `upload`, `respond --image/--stream/--background`) join that tree.
2. **On-disk compatibility is a hard rule.** Both CLIs (and the desktop app)
   read/write the same `~/.lexi-cli/` during the transition; every write preserves
   the existing file formats. No migrations.
3. **Encrypted keys: read + write Fernet-compatible files** in Rust (same KDF/
   format as `encrypt_api_key` in `cli.py`), so no stored key is stranded. OS
   keychain support is a later, separate option.
4. **Interactive shell via `rustyline`**, sharing the existing `~/.lexi-cli/history` file.
5. **Human output is equivalent, not byte-identical** to `rich`; `--raw` / `--json`
   output *is* specified and tested.
6. **Binary-name collision during transition:** the Rust binary stays `lexi`;
   develop with `cargo run -p lexi-cli-rs --`. Only one `lexi` installed at a time.
7. **Fate of `lexi-cli/`:** deleted at cutover (C8). The wheel remains.

## 5. Slice sequence

Each slice = own branch off `main` + PR + green CI, merged in order (the usual habit).
Every slice lands with integration tests (`assert_cmd` against a temp `LEXI_HOME`;
wiremock for anything that hits the network) and `--json` output for its commands.

### C1 — Foundation
- Split `lexi-cli-rs` into a **library** (command tree, parser, dispatcher,
  output) and a thin `lexi` binary, so the GUI console (C9) can run commands
  in-process.
- Surface-coverage test + `docs/surface-matrix.md` (§3), seeded with today's operations.
- clap command-tree structure for the Python grammar; each slice adds only
  commands that fully work (no placeholder stubs).
- Global `--json` flag + a small output module (human table vs JSON).
- First-run seeding in `lexi-host` (embed packaged `providers/` + `default-aliases.json`).
- `version`, consistent exit codes (0 / 1 usage / 2 provider error).

### C2 — Read-only admin
`providers list`, `models list|hosted|default <p>|alias list`, `prompts list`,
`config list`, `api list|show|validate` — all backed by existing host functions.

### C3 — Admin writes
`config set|rm`, `prompts set|rm`, `models add|set|rm|default <p> <m>|alias set|rm`,
`api create|rm|edit`, `providers add|rename|rm`. New host functions:
`delete_config`, provider add/rename/remove, `api edit` param/meta patching.

### C4 — Keys & provider status
`providers api-key set|show [--reveal]|remove` including Fernet read/write
(verify against keys written by the Python CLI), and `providers status`.
`resolve_api_key` learns encrypted files (passphrase prompt / env var).

### C5 — `respond` parity
Named or active prompt, `--model/--temperature/--max_tokens`, dynamic
definition flags from `lexi-core::cli_flags` (`x-cli`), tool presets
(`--web-search`, …), `--set/--unset` path overrides, `--raw`; all composed with the
existing `--stream/--background/--image`. Plus `api preview`.

### C6 — Interactive shell
`rustyline` REPL: history (`↑/↓`, `history [-n] [--clear]`, `#N` re-run), command
aliases (`alias list|add|rm|reset` over `aliases.json`), `help [cmd [sub]]` + topics
(`ai`, `general`, `shortcuts`), `?`, `sh` / `!`, `exit|quit|e|q`.

### C7 — Machine mode
The structured face other programs (and agents) drive:
- `--json` everywhere (from C1 on) with documented shapes.
- `respond --stream --ndjson`: one JSON event per line using the **same event
  shapes as the desktop stream** (`provider_start`, `delta`, `activity`,
  `provider_done`) — one contract for GUI and CLI.
- Artifacts reported as file paths; background jobs `--detach` → id + a
  `jobs status|wait <id>` pair.
- `docs/spec/cli-machine-v0.md`.

### C8 — Cutover
- **Parity harness:** a scripted command list run against both CLIs on the same
  temp `LEXI_HOME`, diffing `--raw`/`--json` output and resulting files. Green
  harness = gate for deletion.
- Port the remaining behavior assertions from `lexi-cli/tests` (141 tests).
- Ship the CLI binary in `release.yml` (macOS universal, Linux, Windows) next to
  the installers; optional Homebrew tap later.
- Regenerate `docs/cli-reference.md` from clap (replaces `generate-cli-reference.py`).
- Delete `lexi-cli/`; CI's Python job becomes wheel-only; update README,
  CLAUDE.md, getting-started.

### C9 — GUI CLI console
- A "CLI" tab in the desktop app: a terminal-style prompt that runs `lexi`
  commands **in-process** through the CLI library (no subprocess, no second
  binary to bundle). Streams output live; shares `~/.lexi-cli/history` and
  command aliases with the terminal CLI.
- Renders human output (ANSI colors/tables → HTML); `--json` output shown as-is.
- `sh` / `!` disabled in the console (it's an app, not a shell).
- Replaces the old bottom "Show CLI Commands" panel, which went silent when the
  Node backend was retired (it relied on an `X-Lexi-Command` header nothing sets now).

## 6. Risks

| Risk | Mitigation |
|---|---|
| File-format drift while both CLIs write `~/.lexi-cli/` | decision 2; round-trip tests per writer; parity harness |
| Fernet/KDF mismatch strands encrypted keys | C4 fixture: key encrypted by the Python CLI must decrypt in Rust (and vice versa) |
| Interactive UX regressions (readline quirks, `rich` tables) | C6 manual checklist run by a human; decision 5 scopes "equivalent" |
| Scope creep into redesigning commands | decision 1; redesigns go in a post-cutover backlog |
| Users with the Python `lexi` installed | release notes: `pip uninstall lexi-cli`; `lexi version` reports which implementation |

## 7. Out of scope

New modalities (audio in, video gen), agentic/tool loops, a persistent `lexi serve`
mode, OS keychain storage, PyPI publishing, wheel bindings over `lexi-host`
(Python `lexi.respond(...)` — backlog; needed for the API surface to reach full
coverage, see §3). All easier *after* consolidation —
each then gets built once.
