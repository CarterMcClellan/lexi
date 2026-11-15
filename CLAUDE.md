# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Architecture Overview

Lexi is a CLI-first tool for working with multiple LLM providers side by side, with a native desktop app. The core is a Rust workspace; the Python CLI is an application on top of it.

- **lexi-core**: Rust engine — the provider-agnostic, pure (no-I/O) schema-driven request composition, response extraction, message IR, and definition validation. Behavioral parity pinned by the `conformance/` oracle.
- **lexi-host**: Rust runtime (I/O layer over the engine) — HTTP/SSE transport, config/key loading, auth, streaming, polling, uploads, and the admin read/write helpers over `~/.lexi-cli/`.
- **lexi-python**: PyO3 wheel wrapping `lexi-core` (`import lexi`); the one engine the Python CLI uses.
- **lexi-cli-rs**: thin Rust CLI binary (`lexi`) over `lexi-host`.
- **lexi-desktop**: native [Tauri](https://tauri.app) app — a thin webview shell whose `/api/*` calls are served in-process by `lexi-host` (see `lexi-desktop/src/backend.rs`). No Node, no Python, no localhost server. Replaced the old Node/Electron `lexi-gui`.
- **lexi-cli**: Python CLI application (src/lexi_cli/), an application on the `lexi` wheel.

### CLI Architecture
- Single-file CLI implementation in `cli.py` (~95KB) with embedded commands, history, and configuration management
- Uses src/ layout with package data including provider configs and default aliases
- Persistent configuration stored in `~/.lexi-cli/` directory structure
- Provider-specific configurations in `~/.lexi-cli/providers/<provider>/` with YAML files for models, aliases, and configs
- Interactive shell with history support and command aliases
- Config-driven API abstraction: provider auth, request body, and response parsing are defined in YAML, not hardcoded

### Configuration Structure
- Main provider config: `~/.lexi-cli/providers/providers-config.yaml`
- Per-provider model configs: `~/.lexi-cli/providers/<provider>/configured-models.yaml`
- Model aliases: `~/.lexi-cli/providers/<provider>/model-aliases.yaml`
- Default model: `~/.lexi-cli/providers/<provider>/default-model.yaml`
- Command aliases: `~/.lexi-cli/aliases.json`
- Prompts stored as individual JSON files in `~/.lexi-cli/prompts/`

## Development Commands

### CLI Development
```bash
# Install CLI in development mode
cd lexi-cli && pip install -e .

# Run CLI directly (multiple options)
lexi                    # If installed
python -m lexi_cli      # From repo root
./cli                   # Using shim script

# Development tools
ruff check src/         # Lint code
ruff format src/        # Format code
mypy src/              # Type checking
pytest                 # Run tests
pytest --cov=lexi_cli  # Run tests with coverage

# Install with crypto support (for API key encryption)
pip install -e ".[crypto]"
```

### Rust workspace (engine / runtime / CLI / desktop)
```bash
cargo test             # all crates
cargo clippy --all-targets   # lint (CI gates -D warnings)
cargo fmt --all        # format

# Desktop app (Tauri)
cargo install tauri-cli --version '^2'   # once
cd lexi-desktop && cargo tauri dev       # run with devtools
cd lexi-desktop && cargo tauri build     # native installer

# The Python wheel builds via maturin, not `cargo build` (PyO3 extension module):
#   pip install ./lexi-python
```

## Key Implementation Details

### Command Structure
All main commands use plural forms (`prompts`, `providers`, `models`, `alias`) and support subcommands. The CLI includes extensive help system accessible via `help <command>` or `help <command> <subcommand>`.

### Provider System
- Three preconfigured providers: OpenAI, Anthropic, NVIDIA
- Each provider has hosted models endpoint support with configurable field mapping
- API keys can be encrypted using cryptography library if installed
- Model metadata and aliases stored separately from main provider config
- Auth headers, request body templates, and response text extraction are all config-driven via `auth`, `request`, and `response` blocks in providers-config.yaml
- Legacy configs using `respond_kind`/`respond_path` are migrated at runtime

### Configuration Seeding
On first run, the CLI seeds `~/.lexi-cli/` from the packaged `providers/` directory and creates default aliases from `default-aliases.json`.

### Dependencies
- Python CLI: Python 3.9+, pyyaml, rich, the `lexi` wheel (optional: cryptography for API key encryption)
- Rust workspace: stable Rust; desktop app needs tauri-cli v2 + the platform webview (macOS WKWebView, Linux webkit2gtk, Windows WebView2)

## File Locations
- Rust engine / runtime / CLI / desktop: `lexi-core/`, `lexi-host/`, `lexi-cli-rs/`, `lexi-desktop/`
- Python wheel: `lexi-python/` (PyO3 over lexi-core)
- Python CLI source: `lexi-cli/src/lexi_cli/`
- Desktop `/api/*` backend: `lexi-desktop/src/backend.rs`; reused frontend snapshot: `lexi-desktop/frontend/index.html`
- Cross-language conformance oracle: `conformance/` (run by `lexi-core/tests/conformance.rs` + `lexi-python/tests/`)
- Provider configs: `providers/` (copied to `~/.lexi-cli/providers/` on first run)
- Package configuration: `lexi-cli/pyproject.toml`, per-crate `Cargo.toml`
