# Lexi

[![CI](https://github.com/badscooter23/lexi/actions/workflows/ci.yml/badge.svg)](https://github.com/badscooter23/lexi/actions/workflows/ci.yml)

Lexi is a CLI-first tool for working with multiple LLM providers side by side, with a native desktop app. It ships with preconfigured support for OpenAI, Anthropic, and NVIDIA, and seeds provider configs into `~/.lexi-cli/` on first run.

## Quick Start

The CLI is a single native binary, `lexi`. Download the archive for your platform
from [GitHub Releases](https://github.com/badscooter23/lexi/releases) (macOS
universal, Linux x86_64/aarch64, Windows x86_64), or build it from source with
stable Rust:

```bash
./install.sh                         # = cargo install --path lexi-cli-rs

lexi                                 # interactive shell
lexi providers list                  # or one-off commands
lexi respond anthropic --text "Explain quicksort in one paragraph"
```

On first run, Lexi seeds `~/.lexi-cli/` (or `$LEXI_HOME`) with provider configs,
model metadata, and default command aliases. It only fills in missing files;
your edits are never overwritten.

## Core Commands

All main commands use **plural forms** and support subcommands. Use `help <command>` for details,
and add `--json` to any command for machine-readable output ([spec](docs/spec/cli-machine-v0.md)).

| Command | Description |
|---|---|
| `respond <provider> [prompt] [--text T] [--model M] [--stream] …` | Send a saved prompt or inline text to a provider |
| `generate <provider> <prompt>` · `upload <provider> <file>` | Generate images to disk · upload to a Files API |
| `jobs status\|wait` | Background jobs (`respond --background --detach`) |
| `prompts list\|set\|rm\|activate` | Manage saved prompts |
| `providers list\|add\|rename\|rm\|status\|api-key` | Manage LLM providers and their keys |
| `models list\|hosted\|add\|set\|rm\|default\|alias` | Manage models for a provider |
| `api list\|show\|create\|edit\|save\|rm\|validate\|preview` | Manage API definitions |
| `alias list\|add\|rm\|reset` | Manage command aliases |
| `config list\|set\|rm` | View or update settings |
| `help`, `history`, `sh`, `version`, `exit` | General commands |

### Examples

```bash
# Save a prompt and send it to Anthropic
prompts set myq --prompt "Explain quicksort in one paragraph"
respond anthropic myq

# Send inline text, streaming, with a specific model and a schema flag
respond openai --text "Hello" --model gpt-4o --stream --temperature 0.2

# Use the active prompt ($$), with web search and citations
respond openai --tool web_search --cite

# List hosted models from a provider
models hosted nvidia --table

# Set a default model
models default anthropic claude-sonnet-4-5-20250929

# Check a provider's status and API key validity
providers status openai

# List available API definitions
api list

# Create a custom API definition from an existing one
api create my-api --from chat-completions
```

### Shortcuts

| Shortcut | Action |
|---|---|
| `?` | Help |
| `!<cmd>` | Run shell command |
| `#<N>` | Re-run history entry |
| Arrow keys | Navigate command history |

## Configuration

All persistent configuration lives in `~/.lexi-cli/`:

```
~/.lexi-cli/
├── apis/                              # Custom API definitions (shipped ones are built in)
├── providers/
│   ├── providers-config.yaml          # Provider URLs, API key refs, API bindings
│   ├── anthropic/
│   │   ├── configured-models.yaml     # Model metadata
│   │   ├── default-model.yaml         # Default model selection
│   │   └── model-aliases.yaml         # Shorthand aliases (e.g., "sonnet")
│   ├── openai/
│   │   └── ...
│   └── nvidia/
│       └── ...
├── prompts/                           # Saved prompts as JSON files
├── aliases.json                       # Command aliases
├── history                            # Shell history
└── config.json                        # CLI settings
```

### API Definitions

Each LLM API style (Chat Completions, Messages, Responses, Image Generations) is defined in a standalone YAML file. The shipped definitions are built into the binary (`lexi api list`, `lexi api show <name>`); custom ones live in `~/.lexi-cli/apis/` (`lexi api create mine --from messages`). An API definition specifies:

- **`auth`** — Header name, value prefix, and any extra headers
- **`request`** — HTTP method, path, body template with `{{model}}`/`{{role}}`/`{{prompt}}` variables, typed parameters with ranges and defaults, and optional tools config
- **`response`** — `text_path` for extracting response text, plus `metadata` paths for token counts, model, and stop reason

Providers reference an API definition by name:

```yaml
providers:
  openai:
    url: https://api.openai.com/v1
    api_key: ${OPENAI_API_KEY}
    api: responses

  anthropic:
    url: https://api.anthropic.com/v1
    api_key: ${ANTHROPIC_API_KEY}
    api: messages

  nvidia:
    url: https://integrate.api.nvidia.com/v1
    api_key: ${NVIDIA_API_KEY}
    api: chat-completions
```

Adding a new provider that uses an existing API style requires only a new entry in `providers-config.yaml` (or `lexi providers add`). Adding a new API style requires only a new YAML file in `apis/` — no code changes needed.

Providers can also include inline `auth`, `request`, or `response` overrides that are deep-merged on top of the referenced API definition.

## Desktop app

`lexi-desktop` is a native [Tauri](https://tauri.app) app — a thin webview shell
over the Rust `lexi-host` runtime and `lexi-core` engine. No Node, no Python, no
localhost server; a native build is ~4 MB. It reuses the same `~/.lexi-cli/`
config as the CLI, and every feature (multi-provider chat, buffered + streaming,
and the full admin surface: providers, models, prompts, config, API definitions)
is served in-process by `lexi-host`.

```bash
cargo install tauri-cli --version '^2'   # once
cd lexi-desktop
cargo tauri dev        # run with devtools
cargo tauri build      # produce a native installer (target/release/bundle/)
```

Prebuilt installers (macOS `.dmg`, Linux `.deb`/`.AppImage`, Windows `.msi`) are
attached to GitHub Releases. See `lexi-desktop/README.md`.

## Python engine (`import lexi`)

The engine is also packaged as an `abi3` Python wheel (a PyO3 binding over the
Rust core — one engine, not a reimplementation). Prebuilt wheels (macOS, Linux,
Windows) and an sdist are attached to each GitHub Release; download the wheel for
your platform and `pip install ./lexi-*.whl`, then `import lexi`. See
`lexi-python/README.md`. (Not on PyPI — the `lexi` name there is taken.)

## Development

```bash
# Rust workspace (engine / runtime / CLI / desktop)
cargo test             # all crates
cargo clippy --all-targets
cargo fmt --all
cargo run -p lexi-cli-rs -- providers list   # run the CLI from source

# Python wheel (engine bindings)
pip install ./lexi-python pytest pyyaml && pytest lexi-python/tests
```

## Documentation

- [Getting Started](docs/getting-started.md) — Install, configure, and send your first prompt
- [CLI Reference](docs/cli-reference.md) — Complete command reference (generated from the CLI)
- [CLI machine interface](docs/spec/cli-machine-v0.md) — The `--json` contract for scripts and agents
- [API Definitions](docs/api-definitions.md) — How the config-driven API abstraction works
- [Architecture](docs/architecture.md) — System design and developer internals

## License

MIT
