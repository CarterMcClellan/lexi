# Architecture

This document describes Lexi's internal architecture for developers working on the codebase.

## System Overview

```
┌──────────────────────────────────────────────────────────────┐
│                          User                                │
│                                                              │
│  CLI (lexi, lexi-cli-rs) ──► lexi-host ────┐                 │
│                                            ├──► lexi-core    │
│  Desktop app (lexi-desktop, Tauri)         │    (pure engine)│
│    webview → invoke → backend.rs → lexi-host                 │
│                                            │                 │
│  Python library (lexi wheel, PyO3) ────────┘                 │
└──────────────────────────────────────────────────────────────┘
                              │
                    ┌─────────┴──────────┐
                    │  ~/.lexi-cli/      │
                    │  ($LEXI_HOME)      │
                    │  ├── apis/ (custom)│
                    │  ├── providers/    │
                    │  ├── prompts/      │
                    │  ├── aliases.json  │
                    │  ├── config.json   │
                    │  └── history       │
                    └────────────────────┘
                              │
                    ┌─────────┴──────────┐
                    │   LLM Provider     │
                    │   APIs             │
                    │   (OpenAI,         │
                    │    Anthropic,      │
                    │    NVIDIA, ...)    │
                    └────────────────────┘
```

### Components

**lexi-core** (`lexi-core/`) — The Rust engine: pure (no-I/O) schema-driven request composition, response extraction, the message IR, and API-definition validation/preview. The shipped API definitions live in `lexi-core/apis/`. Behavioral parity is pinned by the `conformance/` oracle (run from Rust and the wheel).

**lexi-host** (`lexi-host/`) — The Rust runtime (I/O layer): HTTP/SSE transport, config/key loading, auth, streaming, polling, uploads, hosted-model discovery, first-run seeding, and the admin read/write helpers over `~/.lexi-cli/`.

**lexi-cli-rs** (`lexi-cli-rs/`) — The `lexi` CLI: a thin command layer (clap) over `lexi-host`, plus the interactive shell, command aliases, and history.

**lexi-desktop** (`lexi-desktop/`) — A native Tauri app: a thin webview shell whose reused frontend's `/api/*` calls route (via a `fetch`-shim → Tauri `invoke`) to `src/backend.rs`, which dispatches to `lexi-host` in-process. No Node, no Python, no localhost server. Replaced the old Node/Electron `lexi-gui`.

**lexi-python** (`lexi-python/`) — A PyO3 wheel (`import lexi`): the Python library. It wraps both the engine (`lexi-core`: compose/extract/validate) and the runtime (`lexi-host`: `lexi.respond`, `lexi.stream`, `lexi.submit`, `lexi.jobs`, `lexi.generate`, `lexi.upload`, and the admin namespaces `lexi.providers` / `models` / `prompts` / `config` / `apis`), sharing `~/.lexi-cli` with the CLI and returning the CLI's `--json` shapes.

**~/.lexi-cli/** — All persistent state (YAML/JSON), overridable with `$LEXI_HOME`. Every surface reads and writes the same files, so the CLI and desktop app share one configuration.

## CLI Internals

### Entry Point

`lexi-cli-rs/src/main.rs` wires argv and stdio to `lexi_cli::run_with()` (`src/lib.rs`), which:
1. Calls `lexi_host::ensure_home()` to seed missing defaults (see [Seeding and First Run](#seeding-and-first-run))
2. Expands command aliases from `aliases.json` (`shell::expand_aliases`)
3. Enters the interactive shell (`shell::repl`) when no command is given, or parses the argv with clap and dispatches the command

`run_with()` takes output writers instead of touching process stdio and returns the exit code as a `u8`: `EXIT_OK` (0), `EXIT_FAILURE` (1), or `EXIT_USAGE` (2). `execute_line()` runs one shell line the same way, so tests and in-process hosts run exactly what a terminal user types.

### Command Dispatch

- `src/cli.rs` — The clap command tree (`Cli`, with the global `--json` flag). [CLI Reference](cli-reference.md) is generated from it.
- `src/commands/mod.rs` — `dispatch()` routes each parsed command to its module: `respond.rs`, `chat.rs` (`generate`, `upload`, `version`), `jobs.rs`, `prompts.rs`, `providers.rs`, `models.rs`, `api.rs`, `config.rs`, and `shell_cmds.rs` (`help`, `history`, `alias`, `sh`, `exit`).
- `src/output.rs` — `Io`: every command writes through it, emitting human text or, with `--json`, the machine envelope from [spec/cli-machine-v1.md](spec/cli-machine-v1.md).
- `src/shell.rs` — The rustyline REPL: `?` → `help`, `!cmd` → `sh`, `#N` / `#-1` history replay, alias expansion.
- `src/aliases.rs` / `src/history.rs` — `aliases.json` and `~/.lexi-cli/history`. Aliases expand in the shell and for one-off commands, and never shadow a real command.
- `src/coerce.rs` — Parses `--set path=value` arguments into a path and a type-coerced value.

### Configuration System

Default config values:

| Key | Default |
|---|---|
| `cli_name` | `lexi` |
| `prompt` | `cmd` |
| `prompt_delimiter` | `>` |
| `edit_mode` | `vi` |
| `request_timeout` | `300` (seconds; 0 = no limit) |

Config is read from `~/.lexi-cli/config.json` and layered over these defaults (`commands/config.rs`). The `config list|set|rm` commands modify this file.

## Config-Driven API Abstraction

This is the core architectural pattern. Instead of hardcoding how to call each provider's API, Lexi uses YAML definitions that describe the API contract.

### File Layout

```
lexi-core/apis/                  # Shipped definitions, embedded in the binary
├── chat-completions.yaml        # OpenAI-compatible Chat Completions
├── messages.yaml                # Anthropic Messages
├── responses.yaml               # OpenAI Responses
└── image-generations.yaml       # Image generation

~/.lexi-cli/
├── apis/                        # Custom definitions only (`api create`, `api save`)
└── providers/
    └── providers-config.yaml    # Each provider references an API by name
```

Shipped definitions always load from the embedded copy, so a stale file on disk can never mask them; `~/.lexi-cli/apis/` only adds custom ones (`lexi_host::config::load_api_def`).

### Config Resolution Pipeline

`lexi_host::config::effective_config()` builds the effective config for a provider:

```
Provider Config ──► Has `api: <name>`?
                       │
              ┌────────┴────────┐
              Yes               No
              │                 │
    Load API definition    Has inline `request` block?
    (embedded, else             │
     ~/.lexi-cli/apis/)    ┌────┴────┐
              │            Yes       No
    Deep-merge provider    │         │
    entry on top       Use as-is   Error: provider
              │            │       has no API
              ▼            ▼
          Effective Config
```

**Deep merge** (`config::deep_merge()`) is recursive: nested maps are merged key-by-key, lists and scalars are replaced.

### Request Flow

When `respond <provider> [prompt_name]` runs (`lexi-cli-rs/src/commands/respond.rs` → `lexi-host/src/respond.rs`):

1. **Parse flags** — Schema flags are discovered from the definition's `x-cli` entries (`lexi_core::cli_flags`) and converted to their schema types
2. **`prepare_chat()`** — Loads the provider and its effective config, resolves the model (aliases included), the API key, images, `--params`, tool presets, and `--set`/`--unset` paths. No network I/O, so the composed body is inspectable
3. **`compose_chat_body()`** — Builds a canonical message from the prompt (plus images), lowers it through the definition's `content_mapping`, and calls **`lexi_core::compose_request_body()`**, which walks `request.body_schema` (`x-bind`, `x-render`, `x-include`, defaults), applies tool presets and set/unset paths, and validates the result
4. **`config::resolve_api_key()`** — Key file first, then the config's `api_key` (`${ENV_VAR}` or literal)
5. **`auth::build_headers()`** — Constructs HTTP headers from the `auth` block
6. **`run_chat()`** — HTTP POST to `provider url + request.path`, buffered or streamed (SSE); `--background` submits and polls via `submit_chat()` / `wait_operation()`
7. **Extraction** — `lexi_core::extract_text()` (per `response.text_extract`), plus annotations, activity, and metadata paths

### Key Functions

| Function | Location | Purpose |
|---|---|---|
| `compose_request_body(...)` | `lexi-core/src/compose.rs` | Schema-driven request-body composition |
| `validate_definition(def)` | `lexi-core/src/definition.rs` | Schema validation for API definition files |
| `cli_flags(def)` / `preview_definition(...)` | `lexi-core/src/definition.rs` | CLI flag discovery; `api preview` |
| `get_path` / `set_path` / `unset_path` | `lexi-core/src/path.rs` | Walk dot/bracket paths like `content[0].text` |
| `render_template(template, vars)` | `lexi-core/src/template.rs` | Recursive `{{var}}` substitution |
| `extract_text(...)` | `lexi-core/src/extract.rs` | Response text extraction |
| `load_api_def(name)` | `lexi-host/src/config.rs` | Embedded definition, else custom file |
| `effective_config(provider, cfg)` | `lexi-host/src/config.rs` | API def + provider overrides |
| `resolve_api_key(provider, cfg)` | `lexi-host/src/config.rs` | Key file, then config `api_key` |
| `build_headers(cfg, api_key)` | `lexi-host/src/auth.rs` | Auth headers from config |
| `prepare_chat(...)` / `run_chat(...)` | `lexi-host/src/respond.rs` | Full request/response cycle |
| `ensure_home()` | `lexi-host/src/bootstrap.rs` | First-run seeding |

## Seeding and First Run

`lexi_host::ensure_home()` (`lexi-host/src/bootstrap.rs`) runs on every startup and writes the defaults embedded in the binary into `$LEXI_HOME` (default `~/.lexi-cli/`):

1. `providers/providers-config.yaml`
2. For each shipped provider (openai, anthropic, nvidia): `configured-models.yaml`, `model-aliases.yaml`, `default-model.yaml`
3. `aliases.json` from `default-aliases.json` (also reseeded if the file exists but is empty)

API definitions are **not** seeded: the shipped ones resolve from the binary, and `~/.lexi-cli/apis/` holds custom definitions only.

**Key principle:** Existing files are never overwritten. If `~/.lexi-cli/` already has a file, the seeder skips it. This means user customizations are preserved across upgrades.

## Legacy Formats

The old provider config format (`respond_kind` / `respond_path`) and the old API-definition keys (`request.body`, `request.parameters`, `response.text_path`, …) are no longer supported. There is no runtime migration: `api validate` reports legacy keys as errors, and a provider needs an `api: <name>` reference (or an inline `request` block):

```yaml
providers:
  openai:
    url: https://api.openai.com/v1
    api: responses
```

## Desktop App Architecture

`lexi-desktop` is a native [Tauri](https://tauri.app) v2 app — a thin webview shell over `lexi-host`, with **no** Node, Python, or localhost server. A native build is ~4 MB.

### Frontend (`lexi-desktop/frontend/index.html`)

A snapshot of the original single-page UI. It still speaks the old Node server's `/api/*` HTTP contract, but a small `fetch`-shim injected into the page reroutes every `/api/*` call to a Tauri command (`invoke('api', …)`), and streaming (`/api/respond/stream`) to a `ReadableStream` fed by a Tauri `Channel`.

### Backend (`lexi-desktop/src/backend.rs`)

The `api` / `api_stream` Tauri commands dispatch here. `handle(method, path, body)` matches each `/api/*` route and serves it **in-process** from `lexi-host` — chat (buffered + streaming), providers, models (incl. hosted discovery), prompts, config, and the full API-definition surface (list/show/raw/validate/create/save/delete/preview). Response shapes match the original Node handlers, so the reused frontend needs no changes.

### Packaging

`cargo tauri build` produces native installers; `.github/workflows/release.yml` builds macOS/Linux/Windows installers on a `v*` tag via `tauri-action`. See `lexi-desktop/README.md`.

## Testing

```bash
cargo test                  # All crates
cargo test -p lexi-cli-rs   # Just the CLI
```

- **CLI** — Integration tests in `lexi-cli-rs/tests/` run the real `lexi` binary against a temporary `LEXI_HOME` and a wiremock provider (respond, jobs, admin reads/writes, shell, the surface matrix). `cli_reference.rs` checks that [cli-reference.md](cli-reference.md) matches the command tree; regenerate with `LEXI_UPDATE_DOCS=1 cargo test -p lexi-cli-rs --test cli_reference`.
- **Runtime** — `lexi-host` tests exercise the HTTP pipeline against a mock server.
- **Conformance** — The cross-language oracle in `conformance/` is run by `lexi-core/tests/conformance.rs` and, from the wheel, by `lexi-python/tests/`.

Commands return exit codes rather than exiting the process, so tests assert on the code and captured output.

## CI/CD

GitHub Actions:
- **`ci.yml`** — `cargo fmt --check`, `cargo clippy -D warnings`, and tests for the engine (conformance), the wheel (conformance from Python), the host (mock-server pipeline), the CLI (binary end-to-end + surface matrix), and a desktop build
- **`cli.yml`** — Builds the `lexi` binary archives (macOS universal, Linux x86_64/aarch64, Windows x86_64) and attaches them to releases
- **`release.yml`** — Desktop installers via `tauri-action` on a `v*` tag
- **`wheels.yml`** — The `lexi` Python wheels (library) attached to the release
