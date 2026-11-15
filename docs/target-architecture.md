# Lexi — Target Architecture & Vision

**Status:** Draft · 2026-11-14 · supersedes the current Python-CLI + Node/Electron-GUI stack over time.
**Scope:** where Lexi is going. For how the system works *today*, see [architecture.md](architecture.md).

---

## 1. Vision

Lexi's core value — the IP — is a **platform/provider-agnostic library** that exposes **one common API** across LLM use cases:

- text → text (today)
- text → image / video
- text → code

Provider-specific API details are captured through a **combination of configuration and extensible code**: declarative config for the easy majority, typed code adapters for the hard cases. Application code written against Lexi is *portable* across providers; swapping or combining providers is a configuration/routing concern, not a rewrite.

The library is the product. The CLI and GUI are **applications built on top of it.**

---

## 2. The three-layer model

```
┌─────────────────────────────────────────────────────────────┐
│  LIBRARY  — lexi-core  (the IP)                               │
│  provider/platform-agnostic abstraction                       │
│  one common API: text→text, text→image/video, text→code       │
│  provider access = configuration + extensible code            │
└───────────────────────────┬───────────────────────────────────┘
                            │ uses + exposes the FULL surface
┌───────────────────────────▼───────────────────────────────────┐
│  CLI  (application)                                             │
│  = the library's complete functionality, nothing hidden        │
│  dual-mode: human output AND machine (structured) output        │
│  usable by people and by other programs                         │
└───────────────────────────┬───────────────────────────────────┘
                            │ uses (drives the machine mode)
┌───────────────────────────▼───────────────────────────────────┐
│  GUI  (application)                                             │
│  Phase I: multi-client, multi-modal chat (text/image/video out) │
│  Phase II: code generation                                      │
│  portable · easy standalone install · multi-OS · native WM      │
└─────────────────────────────────────────────────────────────────┘
```

### Invariants
- **CLI completeness:** nothing is GUI-only. The CLI is the single canonical surface over the library. This forces API completeness, keeps everything scriptable, and gives a free integration-test surface.
- **Structured inter-layer contract (critical):** the GUI drives the CLI's **machine mode**, never scrapes human-formatted output. Human-formatted (pretty/ANSI) output is for people only. See §5.

---

## 3. Technology decisions

| Component | Choice | Distribution / tooling |
|---|---|---|
| **lexi-core** | Rust crate | crates.io; the one implementation (no per-language reimplementation) |
| **lexi-cli** | Rust binary over the core | `cargo install` + prebuilt release binaries (e.g. cargo-dist); single static binary per OS |
| **lexi-gui** | Tauri (Rust shell + reuse existing HTML/JS frontend) | native installers (.dmg / .AppImage/.deb / .msi), built-in auto-updater |
| **Python library** | PyO3 binding over lexi-core | prebuilt **wheels** via maturin + cibuildwheel; `pip install lexi`, no Rust toolchain for users |
| **C / Swift / Kotlin** (future) | C ABI (native) / uniffi | as demand appears |

### Why one Rust core
- **No drift:** one engine, one behavior, exposed everywhere. The "Python version" is a *binding*, not a parallel implementation. (Precedent: pydantic-core, polars, cryptography, tokenizers.)
- **Types for a growing domain:** multimodal forces a typed IR (content parts, operation states, artifacts); the compiler finds every site that must handle a new variant.
- **One language spans library + CLI + GUI:** Tauri's backend is Rust, so the GUI and CLI share the core directly.
- **Performance is NOT the reason** — Lexi is I/O-bound on provider HTTP. The reasons are distribution, type-safety, and single-source-of-truth.

### Portability & build
- Per-OS build via a **CI matrix** (macOS / Linux / Windows). GUI bundles can't be cross-compiled cleanly — each OS builds its own.
- **Signing is the portability tax**, staged by priority order **macOS → Linux → Windows**: macOS Developer ID + notarization; Windows Authenticode (defer, matching priority); Linux ~free.

---

## 4. Core abstraction design

The current config-driven layer (`api_compose` + `apis/*.yaml`) is the seed of this — it already proves that auth/request/response can be *data*. The target generalizes it from string-templating to a **typed intermediate representation (IR)**.

- **Canonical message IR:** a message is an **ordered list of typed content parts** (`Text | Image | Audio | Video | ToolCall | ...`) with roles. **Adopt this now, while still text-only** — today the list holds a single `Text` part. This makes every future modality *additive* instead of a breaking rewrite.
- **Typed artifacts as outputs:** responses are typed (`text | image | audio | video | file-ref | tool-call`) with content-type and inline-or-reference. Binary artifacts are passed as **file references**, not inline blobs, across layer boundaries.
- **Operation lifecycle:** model an `Operation` that covers both **sync** (returns immediately) and **async** (submit → job id → poll → fetch) so video generation does not require re-plumbing. Some providers use webhooks.
- **Capability registry:** declarative, per-model metadata — input modalities, output modalities, context limit, max image size, supported formats, pricing per modality. Enables routing, fallback, validation, and good errors ("this model can't accept images") instead of opaque 4xx.
- **Config + extensible code (hybrid):** declarative config handles the easy ~80% (OpenAI-compatible text, field paths, auth, endpoints); typed code adapters handle the hard ~20% (Files-API upload flows, async video jobs, tool-call loops). **Do not force everything into config.**
- **Extensibility & escape hatches:** open enums + `extra`/`raw` pass-through for provider-specific fields; a **versioned schema spec** with a **conformance test suite** (fixtures) that every implementation/adapter must pass. Portable code uses the common subset; power users drop to provider-specific options.

### Scope guardrail
Stay a **provider-abstraction + capability-routing** library. Do **not** become an agent-orchestration framework (LangChain/LlamaIndex territory). The "coding/agentic" piece is a *minimal, optional* normalized tool-use + loop, not a framework.

---

## 5. The inter-layer contract (structured protocol)

The CLI has two faces:

| Face | Audience | Shape |
|---|---|---|
| **Human mode** | people at a terminal | pretty text, tables, ANSI |
| **Machine mode** | the GUI and other programs | **JSON / NDJSON**, streaming deltas, **artifacts as file references**, async jobs as submit/poll/status |

The GUI consumes the **machine mode**. This preserves "the GUI uses the CLI" while working for binary, streaming, and async — which scraping pretty output cannot.

**Deferred sub-decision:** GUI spawns the CLI **per request** vs. talks to a persistent **`lexi serve`** process over a pipe/socket speaking the same protocol. `serve` is nicer for chat latency/streaming. Either honors the layering; decide later.

---

## 6. Phase plan

**Phase 0 — Foundation (do first, highest leverage):**
- Adopt the canonical typed message IR + capability-registry skeleton **in the current code**, while still text-only (forward-compat).
- Freeze the provider-schema **spec** + build the **conformance test suite** (fixtures from today's providers).
- Scaffold `lexi-core` in Rust; port the text-chat engine, validated against the conformance suite so it provably matches current behavior.
- Stand up the per-OS **CI build matrix**.

**Phase I — Multimodal chat:**
- Multimodal *input* (vision: image/audio in → text out).
- Image *generation* (text → image), typed binary artifacts.
- Files-API upload protocol (large inputs).
- Async *operations* (video generation): the submit/poll/fetch lifecycle.
- GUI: multi-client, multi-modal chat with text/image/video output (Tauri, reusing the HTML frontend; native file dialogs, media display, job progress).

**Phase II — Code generation / tools:**
- Normalize tool-call formats + a minimal agent loop + structured output across providers (largely parallel to Phase I since it's text/JSON).

---

## 7. Transition from today's stack

- The current **Python CLI** stays as the **reference implementation / conformance oracle** while `lexi-core` is built and proven, then is retired (or kept only if a non-wheel use case appears).
- The current **Node/Electron GUI** is replaced by **Tauri**, reusing the existing `index.html` frontend.
- The in-flight **Python-in-Electron bundling** work is a possible **throwaway stopgap** for something shippable short-term — not the destination. Only invest in it deliberately as such.

---

## 8. Open / deferred decisions
- Per-request CLI spawn vs. persistent `lexi serve` (§5).
- Windows code-signing timing (§3) — deferred to match priority order.
- crates.io / PyPI package names and whether the Python package is `lexi` or namespaced.
- Whether/when to add C ABI and uniffi bindings (demand-driven).

---

## 9. Forward-compat principles ("do now, even while text-only")
1. Messages are **lists of typed content parts**, not bare strings.
2. Outputs are **typed artifacts**; binaries pass as **file references**.
3. There is an **Operation** abstraction covering sync *and* async from day one.
4. There is a **capability registry**; nothing assumes every model does everything.
5. The schema is a **versioned spec** backed by a **conformance suite**.
6. The library boundary is a **structured protocol**, never human-formatted text.
