# CLI Reference

Complete command reference for the Lexi CLI.
This file is auto-generated — run `python docs/generate-cli-reference.py` to update.

---

## Commands

### `prompts`

Manage saved prompts or emit prompt JSON.

**`prompts list`** — List prompts (optionally a single prompt).

```
prompts list [name] [[-r|--raw] [-d|--detailed]]
```

**`prompts set`** — Save or update a prompt definition.

```
prompt set [name] [--prompt TEXT] [[-m|--max_tokens] N] [[-t|--temperature] T] [[-r|--role] ROLE]
```

**`prompts rm`** — Remove a saved prompt.

```
prompt rm <name>
```

---

### `providers`

Manage providers (list/add/rename/rm).

**`providers list`** — List configured providers.

```
providers list [--raw|-r] [--table|-t]
```

**`providers add`** — Add a provider.

```
providers add <name> --url URL --api_key KEY --default_model MODEL
```

**`providers api-key`** — Manage API keys stored in per-provider files.

```
providers api-key set <name> [--value KEY] | providers api-key show <name> [--reveal] | providers api-key remove <name>
```

**`providers rename`** — Rename a provider.

```
providers rename <old> <new>
```

**`providers rm`** — Remove a provider.

```
providers rm <name>
```

**`providers status`** — Check provider status and API key validity.

```
providers status <name> [--raw|-r]
```

---

### `models`

Manage models for a provider (list/add/set/rm).

**`models list`** — List models for a provider.

```
models list <provider> [--raw|-r] [--table|-t] [--details|-d]
```

**`models hosted`** — List models hosted by a provider via its API.

```
models hosted <provider> [--raw|-r] [--table|-t]
```

**`models default`** — View or set the default model for a provider.

```
models default <provider> [model_name]
```

**`models alias`** — Manage model aliases for a provider.

```
models alias list <provider> [--raw|-r] | models alias set <provider> <alias> <model> | models alias rm <provider> <alias>
```

**`models add`** — Add a model to a provider.

```
model add <provider> <model> --name NAME [--description TEXT] [--max_tokens N] [--context_window N] [--default]
```

**`models set`** — Update model metadata.

```
model set <provider> <model> [--name NAME] [--description TEXT] [--max_tokens N] [--context_window N] [--default]
```

**`models rm`** — Remove a model.

```
model rm <provider> <model>
```

---

### `api`

Manage API definitions (list/show/create/edit/rm/validate).

**`api list`** — List API definitions.

```
api list [--raw|-r]
```

**`api show`** — Show an API definition.

```
api show <name> [--raw|-r]
```

**`api create`** — Create a new API definition.

```
api create <name> [--from <existing>] [--path PATH] [--text-path PATH]
```

**`api edit`** — Edit an API definition.

```
api edit <name> [--path PATH] [--text-path PATH] [--set-param NAME:FIELD:TYPE:DEFAULT] [--set-meta KEY:PATH]
```

**`api rm`** — Remove an API definition.

```
api rm <name>
```

**`api validate`** — Validate an API definition.

```
api validate <name>
```

---

### `respond`

Send a prompt to a provider via the Responses API.

```
respond <provider> [prompt_name] [--model M] [--temperature T] [--max_tokens N] [--web-search] [--raw|-r]
```

- Sends the active prompt (or a named prompt) to the specified provider.
- Uses the provider's default model unless `--model` is given.
- Pass `--raw` to print the full JSON response instead of extracted text.

---

## General Commands

### `config`

**`config list`** — Show config values (optionally a single key or raw JSON).

```
config list [--raw|-r|key]
```

**`config set`** — Update a config entry.

```
config set <key> <value>
```

**`config rm`** — Remove a config entry.

```
config rm <key>
```

---

### `alias`

**`alias list`** — List aliases.

```
alias list
```

**`alias add`** — Create/update an alias.

```
alias add <name> <expansion>
```

**`alias rm`** — Remove an alias.

```
alias rm <name>
```

**`alias reset`** — Reset aliases to defaults.

```
alias reset
```

---

### `help`

Show help for commands and topics.

```
help [topic|command [subcommand]]
```

Topics: `ai`, `general`, `shortcuts`

---

### `history`

Show or clear command history.

```
history [-n COUNT] [--reset|--clear]
```

---

### `sh`

Run a shell command.

```
sh <command>
!<command>
```

---

### `version`

Show the CLI version.

```
version
```

---

### `exit`

Exit the CLI.

```
exit | quit | e | q
```

---

## Shortcuts

| Shortcut | Description |
|---|---|
| `?` | Shortcut for 'help'. |
| `!` | Shortcut for 'sh' in interactive mode. |
| `#<N>` | Re-run a command by its history number (#<N>, negatives are relative). |
| `↑ / ↓` | Navigate command history (previous/next). |

