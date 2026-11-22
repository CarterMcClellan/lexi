# Getting Started with Lexi

Lexi is a CLI-first tool for working with multiple LLM providers side by side. This guide walks you through installation, configuration, and sending your first prompt.

## Installation

**Prebuilt binary:** download the `lexi` archive for your platform from [GitHub Releases](https://github.com/badscooter23/lexi/releases) and put the `lexi` binary on your `PATH`:

| Platform | Archive |
|---|---|
| macOS (Apple Silicon + Intel) | `lexi-<version>-macos-universal.tar.gz` |
| Linux x86_64 | `lexi-<version>-linux-x86_64.tar.gz` |
| Linux aarch64 | `lexi-<version>-linux-aarch64.tar.gz` |
| Windows x86_64 | `lexi-<version>-windows-x86_64.zip` |

**From source** (requires stable Rust):

```bash
# Clone the repo
git clone https://github.com/badscooter23/lexi.git
cd lexi

# Build and install the `lexi` binary
cargo install --path lexi-cli-rs

# Verify it works
lexi version
```

Run `lexi` with no command for the interactive shell, or `lexi <command>` for a one-off command:

```bash
lexi                      # Interactive shell
lexi providers list       # One-off command
```

The examples below are written as you would type them inside the shell; outside it, prefix them with `lexi`.

> Looking for the Python library? The `lexi` wheel (`import lexi`, built from `lexi-python/`) is attached to GitHub Releases. It uses the same config as the CLI — `lexi.respond("openai", "hi")` — see `lexi-python/README.md`.

## First Run

Lexi keeps its state in `~/.lexi-cli/` (set `$LEXI_HOME` to use a different directory). On first run it seeds any missing defaults:

- **Provider configs** — `providers/providers-config.yaml`, with preconfigured entries for OpenAI, Anthropic, and NVIDIA
- **Model metadata** — Each provider's `configured-models.yaml`, `default-model.yaml`, and `model-aliases.yaml`
- **Default aliases** — `aliases.json`, with shorthand commands (e.g., `prov` for `providers`, `q` for `exit`)

Existing files are never overwritten. The shipped API definitions (`chat-completions`, `messages`, `responses`, `image-generations`) are built into the binary and are not copied to disk; see [API Definitions](api-definitions.md).

Running `lexi` with no command drops you into an interactive shell:

```
cmd>
```

Type `help` (or `?`) to see available commands, `help <command>` for details on a specific command, or `help general|ai|shortcuts` for a topic.

## Setting Up API Keys

Before you can send prompts, you need API keys from your providers. Lexi supports two methods:

### Environment Variables (recommended)

Set the environment variable for each provider you want to use:

```bash
export OPENAI_API_KEY="sk-..."
export ANTHROPIC_API_KEY="sk-ant-..."
export NVIDIA_API_KEY="nvapi-..."
```

Add these to your shell profile (`~/.zshrc`, `~/.bashrc`, etc.) to persist them.

### Saved API Keys

You can also save keys with Lexi (the desktop app's **Setup Wizard**, on the Providers tab, does this for you):

```bash
# Inside the Lexi shell
providers api-key set openai --value sk-...
providers api-key set anthropic --value sk-ant-...
```

Where a saved key goes:

- **macOS:** the login Keychain (service `Lexi`, account `<provider>`). The key is encrypted at rest, and macOS asks before another app reads it, so a new build of `lexi` or the desktop app may ask once.
- **Windows:** Windows Credential Manager.
- **Linux, or with `LEXI_KEY_STORE=file`:** `~/.lexi-cli/providers/<provider>/api-key`, a plain-text file with owner-only permissions (mode 0600).

On macOS and Windows, a key file saved by an earlier version moves into the keychain the first time Lexi reads it. Without `--value`, the key is prompted for (or read from stdin). A non-default `$LEXI_HOME` gets its own keychain items (account `<provider> (<home>)`).

### Key Resolution Order

When Lexi needs an API key, it checks these sources in order:

1. **Saved key** — the system keychain, then `~/.lexi-cli/providers/<provider>/api-key`
2. **Config `api_key`** — Either a `${VAR}` reference to an environment variable (e.g., `${OPENAI_API_KEY}`) or a literal key in `providers-config.yaml`

The first non-empty value wins. This means a saved key takes precedence over an environment variable.

Apps opened from the macOS Dock or Finder don't see variables set in your shell profile. For the desktop app to use one, run `launchctl setenv OPENAI_API_KEY "…"` and reopen it, or save the key instead.

### Checking API Key Status

View whether a key is configured and where it comes from:

```bash
providers api-key show openai          # Shows the key in use, masked
providers api-key show openai --reveal # Shows the actual key value
providers api-key remove openai        # Deletes the saved key
```

To check that the key actually works (makes a live call to the provider's models endpoint):

```bash
providers status openai
```

`providers status` reports:
- **Source** — Where the key was found (`keychain`, `file`, `env`, `config`, or none)
- **Status** — `ready` (key works), `no api-key` (no key found), or `invalid api-key` (key was rejected by the provider)

## Working with Prompts

Lexi uses a prompt system where you create named prompts that can be sent to any provider.

### Create a Prompt

```bash
prompts set myq --prompt "Explain quicksort in one paragraph"
```

This creates a prompt named `myq` and sets it as the **active prompt** (`$$`).

### View Prompts

```bash
prompts list                    # List all saved prompts
prompts list myq                # Show details for a specific prompt
prompts list --detailed         # Include max_tokens / temperature / role
prompts activate myq            # Make a saved prompt the active one
```

### Prompt Options

Prompts can include parameters that override provider defaults:

```bash
prompts set creative --prompt "Write a haiku about recursion" --temperature 1.2 --max_tokens 100 --role user
```

## Sending Prompts to Providers

The `respond` command sends a prompt to a provider — the words you give, else a saved prompt (`--prompt NAME`), else the active one — and prints the reply. Images in the reply are saved to disk.

```bash
# Send text (`-` reads stdin)
respond anthropic "What is a monad?"

# Send the active prompt to Anthropic
respond anthropic

# Send a saved prompt to OpenAI
respond openai --prompt myq

# Override the model
respond anthropic --model claude-sonnet-4-5-20250929

# Override temperature and max tokens
respond openai --temperature 0.2 --max-tokens 2000

# Stream the response as it arrives
respond anthropic --stream

# Get raw JSON response
respond anthropic --raw
```

Flags like `--temperature` and `--max-tokens` come from the API definition serving the request — `api show <api>` lists them (the `cli=` entries). `respond` also takes `--set path=value` / `--unset path` for arbitrary body fields, `--params FILE`, `--tool <preset>`, `--image URL|PATH`, `--cite`, and `--verbose`. Long-running requests can run with `--background`; add `--detach` to return a job id immediately and check it later with `jobs status <provider> <id>` or `jobs wait <provider> <id>`. See the [CLI Reference](cli-reference.md#respond) for the full list.

Beyond text:

```bash
# Images: --output image picks the provider's image API and default image model
respond openai "a lighthouse at dusk" --output image --out ./images
respond openai "a lighthouse at dusk" --model gpt-image-2.5-flare --size 1536x864
generate openai "a lighthouse at dusk"                     # shortcut for --output image

# Text and an image in one reply
respond openai "Draw a fox and describe it" --tool image_generation

# Embeddings: one vector per text (--json for the vectors)
embed openai "first text" "second text" --dimensions 256
embed nvidia --file docs.txt --input-type passage          # NVIDIA: passage when indexing, query (default) when searching

# NVIDIA image models run on their own host; name one, or ask for --output image
respond nvidia "a lighthouse at dusk" --output image --steps 4
respond nvidia "a lighthouse at dusk" --model stabilityai/sdxl-turbo

upload openai ./report.pdf                                 # Files API upload; prints the file id
```

`models hosted <provider>` lists what the provider serves, with each model's kind and API — filter with `--tag out:image` — and caches the list so those models can be used without configuring them. Where a provider describes its models (Anthropic: image/PDF input, thinking, token limits), that becomes tags too, and `respond` checks requests against it before sending (`--no-validate` skips that).

Each provider can offer several APIs, and a model's **tags** decide which one serves it (`models tags openai gpt-image-2.5-flare` shows why). The shipped OpenAI template serves chat models through the Responses API and `gpt-image-*` models through the Images API, so the image-only flags (`--size`, `--quality`, `--n`, …) appear when the request goes there. Images are saved as `<prompt-words>-<n>.png` and never overwrite an existing file. Tag models a provider can't describe with `models tag <provider> <model> +kind:image-generation` (see `tags` for the vocabulary). `upload` works with providers whose API definition has a `files` block (`responses`, `messages`).

> Upgrading from an older config: entries without `template:` keep working as one API per provider. `providers upgrade` shows how each would move to its shipped template (keeping only the values you changed); `providers upgrade --yes` applies it, backing up the file first.

A provider's APIs are listed by `providers api list <provider>`; add one with `providers api add <provider> <api> --for <tag> --default-model <model>` (e.g. embeddings on a local OpenAI-compatible server), and `providers api rm` removes one (a template's is turned off).

## Managing Providers

### List Providers

```bash
providers list               # Formatted output
providers list --table       # Table format
providers list --raw         # Raw YAML
```

### Add a New Provider

```bash
providers add my-provider --url https://api.example.com/v1 --default_model my-model --api chat-completions
```

`--api` names the API definition the provider speaks (default: `chat-completions`); see the [API Definitions guide](api-definitions.md). If you pass `--api_key`, the key is written to the provider's key file, not to the config.

## Managing Models

### View Available Models

```bash
models list openai                  # Configured models
models hosted openai --table        # All models from the provider's API
```

### Set a Default Model

```bash
models default anthropic claude-sonnet-4-5-20250929
```

### Use Model Aliases

Aliases let you use short names instead of full model IDs:

```bash
models alias set openai gpt4 gpt-4o
respond openai --model gpt4
```

## Command Aliases

Create shortcuts for commands you use often:

```bash
alias add ra respond anthropic
alias add ro respond openai

# Now you can just type:
ra myq
ro --model gpt-4o
```

Aliases expand both in the shell and in one-off commands (`lexi ra myq`), and never shadow a real command. View and manage aliases:

```bash
alias list          # Show all aliases
alias rm ra         # Remove an alias
alias reset         # Reset to the shipped defaults
```

## Configuration

View and modify CLI settings:

```bash
config list                     # Show all settings
config set edit_mode emacs      # Switch to emacs keybindings
config set prompt_delimiter $   # Change the prompt delimiter
```

Default settings:

| Key | Default | Description |
|---|---|---|
| `cli_name` | `lexi` | CLI name |
| `prompt` | `cmd` | Prompt label |
| `prompt_delimiter` | `>` | Character after the prompt label |
| `edit_mode` | `vi` | Input editing mode (`vi` or `emacs`) |

## Shell Integration

Run shell commands without leaving Lexi:

```bash
sh ls -la                    # Run a shell command
!echo "hello"                # Shortcut syntax
```

Re-run previous commands from history (stored in `~/.lexi-cli/history`):

```bash
history                      # Show command history
history -n 5                 # Show last 5 commands
#3                           # Re-run command #3
#-1                          # Re-run the last command
history --clear              # Clear the history
```

## Scripting

Every command accepts the global `--json` flag for machine-readable output, and exits `0` on success, `1` on failure, and `2` on a usage error. See the [machine interface spec](spec/cli-machine-v1.md).

```bash
lexi --json providers list
```

## Next Steps

- [CLI Reference](cli-reference.md) — Full command reference with all options
- [API Definitions](api-definitions.md) — Learn how to add new API styles or customize existing ones
- [Architecture](architecture.md) — Understand how Lexi works under the hood
