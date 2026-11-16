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
| Send a prompt (buffered) | `respond_text`, `respond_with_effective` | `respond` | — | Chat |
| Send a prompt (streaming) | `respond_stream_text`, `respond_stream_with_effective` | `respond` | — | Chat |
| Send a prompt (background job) | `respond_background_text`, `respond_background_with_effective` | `respond` | — | — |
| Attach images to a prompt | `resolve_image`, `resolve_images` | `respond` | — | — |
| Generate images to disk | `generate_images_text`, `generate_images_with_effective` | `generate` | — | — |
| Upload a file | `upload_file`, `upload_file_with_effective` | `upload` | — | — |
| List providers | `provider_names`, `list_providers`, `load_providers` | planned (C2) | — | Providers |
| List configured models | `load_models` | planned (C2) | — | Models |
| List hosted models | `fetch_hosted_models`, `fetch_hosted_models_with` | planned (C2) | — | Models |
| Get / set default model | `default_model`, `set_default_model` | planned (C2 read, C3 write) | — | Models |
| List / set / remove model aliases | `model_aliases`, `set_model_alias`, `delete_model_alias` | planned (C2 read, C3 write) | — | Models |
| Add / update / remove a model | `upsert_model`, `delete_model` | planned (C3) | — | Models |
| Read / write config | `load_config`, `set_config` | planned (C2 read, C3 write) | — | Config |
| List / save / remove / activate prompts | `load_prompts`, `save_prompt`, `delete_prompt`, `set_active_prompt` | planned (C2 read, C3 write) | — | Prompts |
| List / show API definitions | `list_api_defs`, `load_api_raw`, `embedded_api_names` | planned (C2) | — | API definitions |
| Validate an API definition | `validate_api_def` | planned (C2) | — | API definitions |
| Create / save / remove an API definition | `create_api_def`, `save_api_raw`, `delete_api_def` | planned (C3) | — | API definitions |
| Preview an API definition | `preview_api_def` | planned (C5) | — | API definitions |

## Internal

Building blocks used by the operations above, or run implicitly by every surface
(home seeding fills in missing defaults at CLI/GUI startup); not surfaced on their own.

`ensure_home`, `ensure_home_in`, `lexi_home`, `load_api_def`, `deep_merge`, `effective_config`, `resolve_api_key`, `build_headers`

## CLI-only

Commands with no `lexi-host` operation behind them (CLI mechanics, not Lexi
functionality).

`version`
