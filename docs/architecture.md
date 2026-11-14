# Architecture

This document describes Lexi's internal architecture for developers working on the codebase.

## System Overview

```
┌─────────────────────────────────────────────────────────┐
│                      User                               │
│                                                         │
│    Interactive Shell ◄──► lexi CLI (cli.py)             │
│                              │                          │
│    Web Browser ◄──► lexi-gui server (index.js)          │
│                        │                                │
│    Electron App ◄──► lexi-gui server (index.js)         │
│                        │                                │
│                        └──► lexi CLI (subprocess)       │
└─────────────────────────────────────────────────────────┘
                              │
                    ┌─────────┴──────────┐
                    │  ~/.lexi-cli/      │
                    │  ├── apis/          │
                    │  ├── providers/     │
                    │  ├── prompts/       │
                    │  ├── aliases.json   │
                    │  └── config.json    │
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

**lexi-cli** (`lexi-cli/src/lexi_cli/cli.py`) — A single-file Python CLI (~95KB) that implements all core functionality: command parsing, provider management, model management, prompt handling, API calls, and configuration. Runs as an interactive shell or in single-command mode.

**lexi-gui** (`lexi-gui/src/server/index.js`) — A Node.js HTTP server that serves a single-page web UI (`public/index.html`) and proxies user actions to the CLI via subprocess calls. Also has an Electron wrapper (`src/electron/main.js`) for desktop use.

**~/.lexi-cli/** — All persistent state. The CLI reads and writes YAML/JSON files here. The GUI operates through the CLI, so both share the same configuration.

## CLI Internals

### Entry Point

The CLI entry point is the `main()` function, which:
1. Loads config from `~/.lexi-cli/config.json`
2. Calls `seed_default_providers()` to populate defaults on first run
3. Enters the interactive shell loop or processes a single command

`main()` returns an int exit code — it does **not** raise `SystemExit`.

### Command Dispatch

Commands are defined in two dicts:

- `LEXI_COMMANDS` — Top-level command names and descriptions
- `COMMAND_SUBCOMMANDS` — Subcommand definitions with `summary` and `usage` strings

Each top-level command has a `handle_<command>()` function (e.g., `handle_providers()`, `handle_api()`). The main loop tokenizes input, resolves aliases, and dispatches to the appropriate handler.

General commands (`help`, `history`, `config`, `alias`, `sh`, `version`, `exit`) are handled alongside the Lexi-specific commands.

### Configuration System

Default config values:

| Key | Default |
|---|---|
| `cli_name` | `lexi` |
| `prompt` | `cmd` |
| `prompt_delimiter` | `>` |
| `edit_mode` | `vi` |

Config is loaded from `~/.lexi-cli/config.json` at startup and merged with `CONFIG_DEFAULT`. The `config list|set|rm` commands modify this file.

## Config-Driven API Abstraction

This is the core architectural pattern. Instead of hardcoding how to call each provider's API, Lexi uses YAML definitions that describe the API contract.

### File Layout

```
~/.lexi-cli/
├── apis/
│   ├── chat-completions.yaml    # Defines the Chat Completions protocol
│   ├── messages.yaml            # Defines the Anthropic Messages protocol
│   └── responses.yaml           # Defines the OpenAI Responses protocol
└── providers/
    └── providers-config.yaml    # Each provider references an API by name
```

### Config Resolution Pipeline

The `resolve_effective_config()` function builds the effective config for a provider:

```
Provider Config ──► Has `api: <name>`?
                       │
              ┌────────┴────────┐
              Yes               No
              │                 │
    Load API definition    Has `request` block?
              │                 │
    Deep-merge provider    ┌────┴────┐
    overrides on top       Yes       No
              │            │         │
              ▼        Use as-is   Legacy migration
         Effective                (migrate_respond_config)
          Config                      │
                                      ▼
                                 Effective Config
```

**Deep merge** (`deep_merge()`) is recursive: nested dicts are merged key-by-key, lists and scalars are replaced.

### Request Flow

When `respond <provider> [prompt]` runs:

1. **`respond_with_provider()`** — Orchestrates the full request/response cycle
2. **`resolve_effective_config()`** — Loads API def + merges provider overrides
3. **`load_api_key()`** — Reads key from env var or `api-key` file
4. **`build_auth_headers()`** — Constructs HTTP headers from `auth` config
5. **`build_request_body()`** — Assembles JSON body:
   - **`render_body_template()`** — Recursively substitutes `{{model}}`, `{{role}}`, `{{prompt}}` in the body template
   - Maps parameters (temperature, max_tokens) to their `body_field` names
   - Adds tools payload if requested
6. HTTP POST to `provider_url + request_path`
7. **`render_response_output()`** — Extracts text via `response.text_path` using `resolve_path()`
8. **`extract_response_metadata()`** — Pulls token counts, model, stop reason via metadata paths

### Key Functions

| Function | Location | Purpose |
|---|---|---|
| `resolve_path(data, path)` | ~line 380 | Walks dot/bracket paths like `content[0].text` through nested JSON |
| `build_auth_headers(cfg, api_key)` | ~line 400 | Constructs auth headers from config |
| `render_body_template(template, vars)` | ~line 420 | Recursive `{{var}}` substitution in nested structures |
| `build_request_body(...)` | ~line 547 | Assembles the full request JSON body |
| `resolve_effective_config(provider, cfg)` | ~line 633 | API def + provider overrides + legacy fallback |
| `validate_request_params(cfg, ...)` | ~line 660 | Warns if params are outside API-defined ranges |
| `extract_response_metadata(data, cfg)` | ~line 688 | Extracts metadata fields from response JSON |
| `respond_with_provider(...)` | ~line 761 | Full request/response cycle |
| `validate_api_definition(data)` | ~line 300 | Schema validation for API definition files |
| `deep_merge(base, override)` | ~line 624 | Recursive dict merge |
| `migrate_respond_config(provider, cfg)` | ~line 600 | Runtime migration from old config format |

## Seeding and First Run

`seed_default_providers()` runs on every startup and populates `~/.lexi-cli/` from bundled defaults:

1. Copies `providers-config.yaml` if it doesn't exist
2. For each bundled provider directory (openai, anthropic, nvidia):
   - Seeds `configured-models.yaml`, `model-aliases.yaml`, `default-model.yaml`
   - Seeds `api-key` file if present (with chmod 600)
3. Seeds default command aliases from `default-aliases.json`
4. Calls `seed_default_apis()` to copy bundled API definitions to `~/.lexi-cli/apis/`

**Key principle:** Existing files are never overwritten. If `~/.lexi-cli/` already has a file, the seeder skips it. This means user customizations are preserved across upgrades.

## Legacy Migration

The `migrate_respond_config()` function provides runtime backward compatibility for the old provider config format:

**Old format:**
```yaml
providers:
  openai:
    url: https://api.openai.com/v1
    respond_kind: responses
    respond_path: /responses
```

**New format** (API definition reference):
```yaml
providers:
  openai:
    url: https://api.openai.com/v1
    api: responses
```

The migration maps old `respond_kind` values (`responses`, `messages`, `chat-completions`) to their equivalent `auth`, `request`, and `response` blocks at runtime. The on-disk config is not modified — migration happens in memory each time.

## GUI Architecture

### Server (`src/server/index.js`)

The Node.js server:
- Serves static files from `public/`
- Exposes REST endpoints that map to CLI commands
- Executes `lexi <command>` as a subprocess and returns the output
- Routes include: prompts CRUD, provider listing, model management, respond, config

The `LEXI_BIN` environment variable overrides the CLI binary path. Falls back to `lexi` on PATH, then to `lexi-cli/cli` in the repo.

### Frontend (`public/index.html`)

Single-page application — all HTML, CSS, and JavaScript in one file. Communicates with the server via fetch calls to `/api/*` endpoints.

### Electron (`src/electron/main.js`)

Wraps the web UI in a desktop window:
- Starts the Node.js server as a child process
- Opens a `BrowserWindow` pointing to `http://localhost:3000`
- `scripts/prepare-cli.js` bundles the CLI and Python runtime for distribution

## Testing

Tests live in `lexi-cli/tests/`:

- `test_cli.py` — Core CLI tests
- `test_api.py` — API abstraction tests

Key testing patterns:
- `main()` returns an int, does **not** raise `SystemExit` — tests check the return code
- Tests use temp directories to avoid touching `~/.lexi-cli/`
- `print_version()` uses `CONFIG["cli_name"]` which may differ by environment
- Mock subprocess calls for shell command tests

Run tests:
```bash
cd lexi-cli
pytest                  # Run all tests
pytest --cov=lexi_cli   # With coverage
```

## CI/CD

GitHub Actions workflow (`ci.yml`):
- **cli-lint** job — Runs `ruff check` and `ruff format --check`
- **cli-tests** matrix — Runs `pytest` across Python 3.9, 3.11, and 3.12
