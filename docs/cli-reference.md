# CLI Reference

Complete command reference for `lexi`, the Rust CLI.
This file is generated from the command definitions — regenerate with
`LEXI_UPDATE_DOCS=1 cargo test -p lexi-cli-rs --test cli_reference`.

Run `lexi` with no command for the interactive shell. Every command accepts
the global `--json` flag (machine-readable output; see
[spec/cli-machine-v0.md](spec/cli-machine-v0.md)).

---

## `respond`

#### `respond`

Send a prompt to a provider; get back text, images, or both.

respond <provider> [TEXT…|-] [--prompt NAME] [--model M] [--output KIND] [--out DIR] [--role R] [--stream] [--background [--detach]] [--image URL|PATH]... [<schema flags>] [--set path=value]... [--unset path]... [--tool preset[,preset]]... [--params FILE] [--cite] [--raw|-r] [--verbose|-v] [--no-validate]

The words are the prompt (`-` reads stdin); with none, the saved prompt --prompt NAME, else the active one. The model's tags pick the API; --output image picks the provider's image API and default image model. Images are saved to --out (default: here), never overwriting a file.

Schema flags (e.g. --temperature, --size) come from the selected API's definition: see `lexi api show <api>` (the cli= entries).

```
lexi respond <PROVIDER> [ARGS]...
```

- `<PROVIDER>` — Provider name, as in providers-config.yaml
- `<ARGS>` — Prompt name and options (see above)

## `generate`

#### `generate`

Generate image(s) and save them: shortcut for `respond <provider> TEXT --output image`

```
lexi generate [OPTIONS] <PROVIDER> <PROMPT>
```

- `<PROVIDER>` — Provider name
- `<PROMPT>` — The image prompt
- `--model` — Override the provider's default model
- `--out` — Directory to write images into

## `upload`

#### `upload`

Upload a file to a provider's Files API; prints the file id

```
lexi upload <PROVIDER> <FILE>
```

- `<PROVIDER>` — Provider name
- `<FILE>` — Path to the file to upload

## `jobs`

Check or wait for a background job (`respond --background --detach`)

#### `jobs status`

Check a job once; prints its result if it has finished

```
lexi jobs status <PROVIDER> <ID>
```

- `<PROVIDER>`
- `<ID>`

#### `jobs wait`

Wait for a job to finish and print its result

```
lexi jobs wait [OPTIONS] <PROVIDER> <ID>
```

- `<PROVIDER>`
- `<ID>`
- `--interval` — Seconds between polls
- `--timeout` — Give up after this many seconds

## `tags`

#### `tags`

Show the model tag vocabulary (namespaces, kinds and what they imply)

```
lexi tags
```

## `version`

#### `version`

Show the CLI version

```
lexi version
```

## `alias`

Manage command aliases (`aliases.json`)

#### `alias list`

Aliases: `ls`

List aliases (optionally only those for the given commands/aliases)

```
lexi alias list [NAMES]...
```

- `<NAMES>`

#### `alias add`

Aliases: `set`

Create or update an alias: `alias add pl providers list`

```
lexi alias add <NAME> <EXPANSION>...
```

- `<NAME>`
- `<EXPANSION>`

#### `alias rm`

Aliases: `del`, `delete`

Remove an alias

```
lexi alias rm <NAME>
```

- `<NAME>`

#### `alias reset`

Reset aliases to the shipped defaults

```
lexi alias reset
```

## `history`

#### `history`

Show or clear command history

```
lexi history [OPTIONS]
```

- `-n, --number` — Show the last N entries (negative: the first N)
- `--clear` — Clear the history

## `sh`

#### `sh`

Run a shell command (also `!command` in the interactive shell)

```
lexi sh <COMMAND>...
```

- `<COMMAND>`

## `exit`

#### `exit`

Aliases: `quit`

Leave the interactive shell

```
lexi exit
```

## `config`

View or update CLI settings (config.json)

#### `config list`

Aliases: `ls`

Show config values (all, or a single key)

```
lexi config list [OPTIONS] [KEY]
```

- `<KEY>` — Show only this key
- `-r, --raw` — Print the raw config file
- `-v, --value` — With a key: print only its value

#### `config set`

Set a config value (the rest of the line is the value)

```
lexi config set <KEY> <VALUE>...
```

- `<KEY>`
- `<VALUE>`

#### `config rm`

Aliases: `del`, `delete`

Remove a config value

```
lexi config rm <KEY>
```

- `<KEY>`

## `prompts`

Manage saved prompts

#### `prompts list`

Aliases: `ls`

List prompts (or show one). The active prompt is marked `*`

```
lexi prompts list [OPTIONS] [NAME]
```

- `<NAME>` — Show only this prompt
- `-r, --raw` — Print the prompt JSON
- `-d, --detailed` — Include max_tokens / temperature / role
- `-t, --table` — Render as a table

#### `prompts set`

Aliases: `save`

Save or update a prompt: `prompts set [name] [text…] [--prompt TEXT] [-m|--max_tokens N] [-t|--temperature T] [-r|--role ROLE] [--set path=value] [--unset path]`. With one bare word and no name, it is the text of the active prompt. Text `\n` becomes a newline

```
lexi prompts set [ARGS]...
```

- `<ARGS>`

#### `prompts rm`

Aliases: `del`, `delete`

Remove a saved prompt

```
lexi prompts rm <NAME>
```

- `<NAME>`

#### `prompts activate`

Aliases: `use`

Make a saved prompt the active one (copies it into `$$`)

```
lexi prompts activate <NAME>
```

- `<NAME>`

## `providers`

Manage providers

#### `providers list`

Aliases: `ls`

List configured providers

```
lexi providers list [OPTIONS]
```

- `-r, --raw` — Print the raw providers-config.yaml
- `-t, --table` — Render as a table

#### `providers status`

Check a provider's API key (live call to its models endpoint)

```
lexi providers status [OPTIONS] <NAME>
```

- `<NAME>`
- `-r, --raw` — Print {provider, status, source} as JSON

#### `providers add`

Add a provider

```
lexi providers add [OPTIONS] --url <URL> --default_model <DEFAULT_MODEL> <NAME>
```

- `<NAME>`
- `--url` — Base URL, e.g. https://api.example.com/v1
- `--default_model` — The model used when none is given
- `--api_key` — API key (stored in the provider's key file, not the config)
- `--api` — API definition the provider speaks
- `--hosted_models` — Path of the provider's models endpoint

#### `providers api-key set`

Store an API key (prompted for, or read from stdin, without --value)

```
lexi providers api-key set [OPTIONS] <NAME>
```

- `<NAME>`
- `-v, --value`

#### `providers api-key show`

Show the API key in use, masked unless --reveal

```
lexi providers api-key show [OPTIONS] <NAME>
```

- `<NAME>`
- `-r, --reveal`

#### `providers api-key remove`

Delete the stored key file

```
lexi providers api-key remove <NAME>
```

- `<NAME>`

#### `providers rename`

Rename a provider

```
lexi providers rename <OLD> <NEW>
```

- `<OLD>`
- `<NEW>`

#### `providers rm`

Aliases: `del`, `delete`

Remove a provider and its models, aliases and key file

```
lexi providers rm <NAME>
```

- `<NAME>`

## `models`

Manage a provider's models

#### `models list`

Aliases: `ls`

List a provider's configured models

```
lexi models list [OPTIONS] <PROVIDER>
```

- `<PROVIDER>`
- `-r, --raw` — Print the raw configured-models.yaml
- `-a, --aliases` — With --raw: also print the raw model-aliases.yaml
- `-t, --table` — Render as a table
- `-d, --details` — Include every metadata field

#### `models hosted`

List the models a provider hosts (live call to its models endpoint)

```
lexi models hosted [OPTIONS] <PROVIDER>
```

- `<PROVIDER>`
- `-r, --raw` — Print the models as JSON
- `-t, --table` — Render as a table
- `--tag` — Only models with this tag (repeatable: all must match), e.g. out:image
- `--cached` — Use the last fetched listing instead of calling the provider

#### `models default`

Show a provider's default model, or set it

```
lexi models default <PROVIDER> [MODEL]
```

- `<PROVIDER>`
- `<MODEL>` — The model to make the default (must be configured)

#### `models alias list`

Aliases: `ls`

List a provider's model aliases

```
lexi models alias list [OPTIONS] <PROVIDER>
```

- `<PROVIDER>`
- `-r, --raw` — Print the raw model-aliases.yaml

#### `models alias set`

Point an alias at a model

```
lexi models alias set <PROVIDER> <ALIAS> <MODEL>
```

- `<PROVIDER>`
- `<ALIAS>`
- `<MODEL>`

#### `models alias rm`

Aliases: `del`, `delete`

Remove an alias

```
lexi models alias rm <PROVIDER> <ALIAS>
```

- `<PROVIDER>`
- `<ALIAS>`

#### `models add`

Add a model (or update it if it exists)

```
lexi models add [OPTIONS] <PROVIDER> <MODEL>
```

- `<PROVIDER>`
- `<MODEL>`
- `--name` — Display name
- `--description`
- `--max_tokens`
- `--context_window`
- `--default` — Make this the provider's default model

#### `models set`

Update a model's metadata (or add it)

```
lexi models set [OPTIONS] <PROVIDER> <MODEL>
```

- `<PROVIDER>`
- `<MODEL>`
- `--name` — Display name
- `--description`
- `--max_tokens`
- `--context_window`
- `--default` — Make this the provider's default model

#### `models rm`

Aliases: `del`, `delete`

Remove a model

```
lexi models rm <PROVIDER> <MODEL>
```

- `<PROVIDER>`
- `<MODEL>`

#### `models tag`

Add or remove tags on a model id or glob: `+out:image`, `-in:image`.

Tags describe what a model takes, produces and supports; they decide which of the provider's APIs serves it. Your tags win over the provider's tag rules. See `lexi tags` for the vocabulary.

```
lexi models tag [OPTIONS] <PROVIDER> <MODEL> [OPS]...
```

- `<PROVIDER>`
- `<MODEL>` — A model id, or a glob such as "meta/*"
- `<OPS>` — Tag operations: `+tag` (or `tag`) adds, `-tag` removes
- `--clear` — Remove all of your tags on this model or glob

#### `models tags`

Show a model's effective tags, where each came from, and its API

```
lexi models tags <PROVIDER> <MODEL>
```

- `<PROVIDER>`
- `<MODEL>`

## `api`

Manage API definitions

#### `api list`

Aliases: `ls`

List API definitions (shipped and custom)

```
lexi api list [OPTIONS]
```

- `-r, --raw` — Print every definition as JSON

#### `api show`

Show an API definition

```
lexi api show [OPTIONS] <NAME>
```

- `<NAME>`
- `-r, --raw` — Print the definition as JSON

#### `api validate`

Validate an API definition (exit 1 if invalid)

```
lexi api validate <NAME>
```

- `<NAME>`

#### `api create`

Create a custom API definition (scaffold, or a copy of --from)

```
lexi api create [OPTIONS] <NAME>
```

- `<NAME>`
- `--from` — Copy this definition (shipped or custom)
- `--path` — Request path
- `--text-path` — Response text path (switches to simple text extraction)

#### `api edit`

Edit a custom API definition

```
lexi api edit [OPTIONS] <NAME>
```

- `<NAME>`
- `--path`
- `--text-path`
- `--set-meta` — Add response metadata: KEY:PATH. Repeatable

#### `api rm`

Aliases: `del`, `delete`

Remove a custom API definition

```
lexi api rm <NAME>
```

- `<NAME>`

#### `api preview`

Preview a definition: validation, CLI flags, tool presets, and a sample composed request body (always JSON)

```
lexi api preview [OPTIONS] [NAME]
```

- `<NAME>` — A definition by name (or use --file)
- `--file` — Preview a YAML file instead of a saved definition
- `--prompt`
- `--model`
- `--role`

#### `api save`

Save a YAML file as a custom definition (prints validation errors)

```
lexi api save <NAME> <FILE>
```

- `<NAME>`
- `<FILE>`

## Shortcuts (interactive shell)

| Shortcut | Description |
|---|---|
| `?` | Shortcut for `help`. |
| `!<command>` | Run a shell command. |
| `#<N>` | Re-run history entry N (`#-1` is the last). |
| `↑ / ↓` | Navigate command history. |
