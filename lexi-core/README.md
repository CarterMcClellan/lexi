# lexi-core

The provider-agnostic engine for the Lexi API-definition format, in Rust.

This is a pure port of the Python `lexi_cli.api_compose` + `lexi_cli.message_ir`
engines — schema-driven request-body composition and response extraction, plus
the canonical message IR. It is the first step toward the target architecture
(see [`docs/target-architecture.md`](../docs/target-architecture.md)): one Rust
core that will back the CLI, a Tauri GUI, and a Python wheel.

## What it does

- **Compose** a provider request body from a schema-driven API definition
  (`compose_request_body`) — bindings, templates, include-modes, tool presets,
  set/unset paths, and validation.
- **Extract** results from a provider response (`extract_text`,
  `summarize_activity`, `extract_annotations`, `render_annotations`).
- **Message IR** (`message_ir`) — a message as typed content parts, lowering
  losslessly to template bindings for text chat.

Behavior is specified by [`docs/spec/api-definition-v0.md`](../docs/spec/api-definition-v0.md)
and [`docs/spec/message-ir-v0.md`](../docs/spec/message-ir-v0.md).

## Parity

Correctness is pinned by the shared, language-neutral conformance suite under
[`/conformance`](../conformance/README.md). The same fixtures run against both
the Python engine (`lexi-cli/tests/test_conformance.py`) and this crate
(`tests/conformance.rs`), so passing both proves the two engines agree.

```sh
cargo test -p lexi-core        # run the conformance suite
cargo clippy -p lexi-core --all-targets -- -D warnings
cargo fmt -p lexi-core -- --check
```

> Values are `serde_json::Value`; API definitions are plain data. `serde_yaml`
> is a dev-only dependency used by the test harness to load the bundled API
> definitions under `apis/`.
