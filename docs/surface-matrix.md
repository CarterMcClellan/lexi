# Lexi Surface Matrix

Every user-facing operation lives once, in `lexi-host`, and is exposed on each
surface (see [cli-consolidation-plan.md](cli-consolidation-plan.md) §3). This
file is the checklist — and it is **checked by CI**
(`lexi-cli-rs/tests/surface_matrix.rs`):

- every top-level `pub fn` in `lexi-host/src/*.rs` must appear below, either in an
  **Operations** row or under **Internal**;
- every name listed must exist in `lexi-host`;
- every **CLI** cell must be one or more real `lexi` command paths (comma-separated,
  in backticks), or `planned (Cn)` for a consolidation slice not yet landed;
- every `lexi` command must appear in an Operations row or under **CLI-only**.

Cells: `—` = not exposed on that surface. The **Python API** column is tracked,
not yet enforced (the wheel wraps `lexi-core` only; wrapping `lexi-host` is the
next plan). **GUI** names the desktop tab.

## Operations

| Operation | lexi-host | CLI | Python API | GUI |
|---|---|---|---|---|
| Send a saved / inline prompt with full options (model aliases, schema flags, presets, `--set`, params) | `prepare_chat`, `run_chat`, `compose_chat_body` | `respond` | — | — |
| Send a prompt (buffered) | `respond_text`, `respond_with_effective` | `respond` | — | Chat |
| Send a prompt (streaming) | `respond_stream_text`, `respond_stream_with_effective` | `respond` | — | Chat |
| Send a prompt (background job) | `respond_background_text`, `respond_background_with_effective` | `respond` | — | — |
| Attach images to a prompt | `resolve_image`, `resolve_images` | `respond` | — | — |
| Generate images to disk | `generate_images_text`, `generate_images_with_effective` | `generate` | — | — |
| Upload a file | `upload_file`, `upload_file_with_effective` | `upload` | — | — |
| List providers | `provider_names`, `list_providers`, `load_providers`, `api_key_source` | `providers list` | — | Providers |
| Add / rename / remove a provider | `add_provider`, `rename_provider`, `remove_provider` | `providers add`, `providers rename`, `providers rm` | — | — |
| Store / show / remove an API key | `set_api_key`, `remove_api_key` | `providers api-key set`, `providers api-key show`, `providers api-key remove` | — | — |
| Check an API key | `check_api_key` | `providers status` | — | Providers |
| List configured models | `load_models` | `models list` | — | Models |
| List hosted models | `fetch_hosted_models`, `fetch_hosted_models_with` | `models hosted` | — | Models |
| Get / set default model | `default_model`, `set_default_model` | `models default` | — | Models |
| List model aliases | `model_aliases` | `models alias list` | — | Models |
| Set / remove model aliases | `set_model_alias`, `delete_model_alias` | `models alias set`, `models alias rm` | — | Models |
| Add / update / remove a model | `upsert_model`, `delete_model` | `models add`, `models set`, `models rm` | — | Models |
| Read config | `load_config` | `config list` | — | Config |
| Write config | `set_config`, `delete_config` | `config set`, `config rm` | — | Config |
| List prompts | `load_prompts` | `prompts list` | — | Prompts |
| Save / remove / activate prompts | `save_prompt`, `delete_prompt`, `set_active_prompt` | `prompts set`, `prompts rm`, `prompts activate` | — | Prompts |
| List / show API definitions | `list_api_defs`, `load_api_raw`, `embedded_api_names` | `api list`, `api show` | — | API definitions |
| Validate an API definition | `validate_api_def` | `api validate` | — | API definitions |
| Create / edit / remove an API definition | `create_api_def`, `edit_api_def`, `delete_api_def` | `api create`, `api edit`, `api rm` | — | API definitions |
| Save an API definition's raw YAML | `save_api_raw` | `api save` | — | API definitions |
| Preview an API definition | `preview_api_def` | `api preview` | — | API definitions |

## Internal

Building blocks used by the operations above, or run implicitly by every surface
(home seeding fills in missing defaults at CLI/GUI startup); not surfaced on their own.

`ensure_home`, `ensure_home_in`, `lexi_home`, `load_api_def`, `deep_merge`, `effective_config`, `resolve_api_key`, `build_headers`

## CLI-only

Commands with no `lexi-host` operation behind them (CLI mechanics, not Lexi
functionality).

`version`
