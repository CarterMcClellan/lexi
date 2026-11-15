# Lexi Host Phase — Plan

**Status:** Draft · 2026-11-15 · follows the completed engine track (Phase 0 + Phase I).
**Scope:** turning the pure `lexi-core` engine into working applications — the I/O
layer plus the CLI, Python wheel, and Tauri GUI surfaces. See
[target-architecture.md](target-architecture.md) for the end-state.

---

## 1. Where the engine left off

`lexi-core` (Rust) and `lexi_cli` (Python) implement the full, parity-proven
**data plane**: request composition, response extraction (text/activity/
annotations/metadata/artifacts), the message IR (text + image: url/base64/file),
a capability registry, the async operation lifecycle, and Files-API upload
interpretation. Six conformance categories pin every behavior in both languages.

`lexi-core` is deliberately **pure** — no HTTP, no disk, no clock. The current
Python CLI does real HTTP for **text chat (sync + streaming)** only; everything
multimodal/async/artifact/upload is **engine-ready but not wired to any I/O**.

## 2. The host = the I/O layer

```
                 ┌─────────────────────────────────────────────┐
                 │  RUNTIME (new — the side-effectful layer)   │
                 │  HTTP transport · auth/key loading ·        │
                 │  SSE streaming · operation polling loop ·   │
                 │  multipart upload · artifact save/download ·│
                 │  provider/config loading · capability gating│
                 └───────────────┬─────────────────────────────┘
                     uses lexi-core for every pure decision
        ┌────────────────────────┼────────────────────────┐
     CLI (thin shell)      Python wheel (PyO3)        Tauri GUI
```

### Host capabilities (all engine-ready; "wired?" = driven by real I/O today)
| # | Capability | Engine piece | Wired? |
|---|---|---|---|
| 1 | HTTP transport | `compose_request_body` + `extract_*` | ✅ Python CLI (text) |
| 2 | SSE streaming | `response.stream` | ✅ Python CLI |
| 3 | Operation polling loop | `read_operation` / `poll_path` | ❌ |
| 4 | Multipart upload | `read_upload` / `upload_path` | ❌ |
| 5 | Artifact handling | `extract_artifacts` → download/decode → save | ❌ |
| 6 | Capability gating | `validate_message` | ❌ |
| 7 | Config / providers / keys | `resolve_effective_config` | ✅ Python CLI |

## 3. Slice sequence

Decision (2026-11-15): **start with the Python wheel (H1).** It proves the
multi-language-library thesis (the core IP), lets the Python CLI shed its
duplicate engine, and is verifiable against the existing conformance suite.

- **H1 — Python wheel (PyO3) over `lexi-core`.** *This phase's first slice.*
- **H1b — Retrofit the Python CLI** to consume the wheel, retiring the duplicate
  Python `api_compose` / `message_ir` / `capabilities` / `operations` / `uploads`.
- **H2 — Rust `lexi-host` runtime + thin Rust CLI** (HTTP, streaming, polling,
  uploads, artifacts over `lexi-core`); text chat end-to-end first.
- **H3 — Wire the remaining features** (background polling, uploads, artifact
  saving, multimodal input UX) into the chosen host(s).
- **H4 — Tauri GUI** over the runtime, reusing the existing `index.html`.

## 4. H1 in detail — Python wheel

**Deliverable:** a `maturin`-built wheel exposing `lexi-core` to Python, proven
to match the Rust/Python engines via the shared conformance suite run *from the
wheel*.

- **Bindings crate** (`lexi-python/`, PyO3): thin wrappers over `lexi-core`,
  converting Python objects ↔ `serde_json::Value` (via `pythonize`). Exposes
  `compose_request_body`, the `extract_*` family, `message` lowering,
  `resolve_capabilities` / `validate_message`, `read_operation` / `poll_path`,
  `read_upload` / `upload_path`.
- **Packaging**: `maturin` + a `pyproject.toml`; local wheel build on this
  platform first. Multi-platform wheels (`cibuildwheel`) are a follow-up.
- **Verification**: a new Python conformance runner that imports the **wheel**
  and executes the same `conformance/` fixtures — a third engine behind the same
  oracle.
- **Non-goals for H1**: the CLI retrofit (that is H1b, kept separate so the
  working CLI's 297 tests aren't destabilized in the same step); multi-platform
  wheel CI.

## 5. Testing strategy shift

Pure parity stays in `conformance/` (now also run from the wheel). The *I/O*
layer (H2+) needs a different harness: **recorded request/response cassettes or
a mock HTTP server**, so transport + extraction are verified without live calls.
Live provider calls become opt-in smoke tests.

## 6. Open decisions
- **Package / import name** for the wheel (`lexi` vs `lexi_core`); the CLI
  console command is already `lexi` and its Python package is `lexi_cli`.
- **Bindings surface**: dict-in/dict-out (via `pythonize`) vs. typed Python
  classes mirroring the IR. Start with dict-in/dict-out (matches the engine).
- **Async runtime for H2**: `tokio` + `reqwest` (needed for GUI streaming /
  concurrent providers) vs. blocking. Decide at H2.
- **Multi-platform wheel + Tauri signing/notarization** — staged later (macOS →
  Linux → Windows), per target-architecture.
