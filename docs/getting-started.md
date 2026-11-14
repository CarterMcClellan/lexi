# Getting Started with Lexi

Lexi is a CLI-first tool for working with multiple LLM providers side by side. This guide walks you through installation, configuration, and sending your first prompt.

## Installation

**Requirements:** Python 3.9+

```bash
# Clone the repo
git clone https://github.com/badscooter23/lexi.git
cd lexi

# Install the CLI in editable mode
cd lexi-cli && pip install -e .

# Verify it works
lexi version
```

You can also install with encryption support for API keys:

```bash
pip install -e ".[crypto]"
```

Alternative ways to run the CLI:

```bash
lexi                    # Installed console script
python -m lexi_cli      # Module invocation
./cli                   # Shim script in lexi-cli/
```

## First Run

On first launch, Lexi creates `~/.lexi-cli/` and seeds it with:

- **Provider configs** — Preconfigured entries for OpenAI, Anthropic, and NVIDIA
- **API definitions** — YAML files describing how to call each provider's API
- **Model metadata** — Known models, aliases, and defaults for each provider
- **Default aliases** — Shorthand commands (e.g., `r` for `respond`)

You'll land in an interactive shell:

```
lexi cmd>
```

Type `help` to see available commands, or `help <command>` for details on a specific command.

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

### Stored API Keys

You can also store keys in Lexi's config directory:

```bash
# Inside the Lexi shell
providers api-key set openai --value sk-...
providers api-key set anthropic --value sk-ant-...
```

Stored keys are saved to `~/.lexi-cli/providers/<provider>/api-key` with restricted permissions (chmod 600). If the `cryptography` package is installed, keys are encrypted at rest.

### Key Resolution Order

When Lexi needs an API key, it checks these sources in order:

1. **Stored key file** — `~/.lexi-cli/providers/<provider>/api-key`
2. **Environment variable** — The `${VAR}` referenced in `providers-config.yaml` (e.g., `${OPENAI_API_KEY}`)

The first non-empty value wins. This means a stored key file takes precedence over an environment variable.

### Checking API Key Status

View whether a key is configured and where it comes from:

```bash
providers api-key show openai          # Shows if a key file exists
providers api-key show openai --reveal # Shows the actual key value
```

To check that the key actually works (makes a test API call):

```bash
providers status openai
```

`providers status` reports:
- **Source** — Where the key was found (`file`, `env`, or none)
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
prompts list --detailed         # Show all prompts with full details
```

### Prompt Options

Prompts can include parameters that override provider defaults:

```bash
prompts set creative --prompt "Write a haiku about recursion" --temperature 1.2 --max_tokens 100 --role user
```

## Sending Prompts to Providers

The `respond` command sends the active prompt (or a named prompt) to a provider:

```bash
# Send the active prompt to Anthropic
respond anthropic

# Send a named prompt to OpenAI
respond openai myq

# Override the model
respond anthropic --model claude-sonnet-4-5-20250929

# Override temperature and max tokens
respond openai --temperature 0.2 --max_tokens 2000

# Get raw JSON response
respond anthropic --raw
```

## Managing Providers

### List Providers

```bash
providers list               # Formatted output
providers list --table       # Table format
providers list --raw         # Raw YAML
```

### Add a New Provider

```bash
providers add my-provider --url https://api.example.com/v1 --api_key '${MY_API_KEY}' --default_model my-model
```

The provider will need an API definition — see the [API Definitions guide](api-definitions.md) for how to set one up.

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
alias add ra "respond anthropic"
alias add ro "respond openai"

# Now you can just type:
ra myq
ro --model gpt-4o
```

View and manage aliases:

```bash
alias list          # Show all aliases
alias rm ra         # Remove an alias
alias reset         # Reset to defaults
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
| `cli_name` | `lexi` | Name shown in the prompt |
| `prompt` | `cmd` | Prompt label |
| `prompt_delimiter` | `>` | Character after the prompt label |
| `edit_mode` | `vi` | Input editing mode (`vi` or `emacs`) |

## Shell Integration

Run shell commands without leaving Lexi:

```bash
sh ls -la                    # Run a shell command
!echo "hello"                # Shortcut syntax
```

Re-run previous commands from history:

```bash
history                      # Show command history
history -n 5                 # Show last 5 commands
#3                           # Re-run command #3
#-1                          # Re-run the last command
```

## Next Steps

- [CLI Reference](cli-reference.md) — Full command reference with all options
- [API Definitions](api-definitions.md) — Learn how to add new API styles or customize existing ones
- [Architecture](architecture.md) — Understand how Lexi works under the hood
