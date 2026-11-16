# Providers, APIs and Models — Design

**Status:** proposal for review · 2026-11-16
**Scope:** how Lexi configures providers that offer several kinds of model
(text, image, embeddings, …) over several APIs, classifies their models, and
exposes one `respond` for every kind of output.
**Priority:** OpenAI and Anthropic (P1), NVIDIA (P2), and new providers (Gemini, …)
added by configuration, not code.
Model ids in examples are illustrative; templates ship whatever is current.

## 1. Problem

Today a provider has exactly one `api:`, one `url`, one `default_model`:

```yaml
openai: { url: https://api.openai.com/v1, api: responses, default_model: gpt-4o }
```

That breaks as soon as one provider offers more than one kind of model:

- **OpenAI**: chat models use `/responses`; GPT Image models use only
  `/images/generations` (they're rejected by Responses/Chat Completions). Today
  that needs a second provider, `openai-images`, with the same key and URL.
- **NVIDIA**: one `/v1/models` listing mixes chat, vision, **9 embedding models**,
  safety classifiers and detectors (observed with a real key: 33 models). About a
  third of them would fail in `respond`, and Lexi can't tell which. Image and
  rerank models live on **other hosts**.
- **Commands**: `respond` (text) and `generate` (images) are the same operation
  (prompt → provider → output) split by output type, and neither can handle a
  response with both text *and* images (e.g. OpenAI's image-generation tool).

And Lexi can't learn a model's kind from providers uniformly:

| Provider | `GET /models` per model | Classifiable by |
|---|---|---|
| OpenAI | `id, object, created, owned_by, shutdown_date` | name (`gpt-image-*`, `text-embedding-*`, `tts-*`, …) — reliable |
| Anthropic | `id, display_name, created_at, max_input_tokens, max_tokens, capabilities{image_input, pdf_input, thinking, effort, structured_outputs, …}` | real data; all models are Messages / text-out |
| NVIDIA | `id, object, created` (constant, meaningless), `owned_by`; *one* model also had `model_type, context_window, supports_tools, supports_thinking` | name, loosely (`*embed*`, `*guard*`); partial data |
| Gemini (future) | `supportedGenerationMethods`, `inputTokenLimit`, `outputTokenLimit`, … | real data |

**Requirement:** everything provider-specific lives in **configuration**
(provider entries + API definitions), so adding or fixing a provider is a YAML
change. Code knows concepts, never provider names.

## 2. Concepts

Three things, kept separate:

| Concept | Is | Defined in |
|---|---|---|
| **API definition** | One wire protocol: endpoint path, request schema, response extraction, streaming, and **what it carries** (`capabilities.input` / `.output`) | `lexi-core/apis/*.yaml` (shipped) or `apis/` (custom) — unchanged format, small additions (§4) |
| **Provider** | An account: identity, auth, base URL, **the APIs it offers** (bindings), its **model catalog**, and **how to classify** its models | `providers/providers-config.yaml`, over a shipped **provider template** (§3.4) |
| **Model** | An id the provider serves, resolved to **one binding** and a **capability descriptor** | classification (§5): config → patterns → catalog data → default |

**Output kinds** — the vocabulary that ties them together: `text`, `image`,
later `audio`, `vector` (embeddings), `video`. An API definition declares which
it produces; a command or flag asks for one; a model resolves to one binding.

## 3. Provider configuration

### 3.1 Bindings: the APIs a provider offers

```yaml
providers:
  openai:
    template: openai                 # shipped defaults (§3.4); everything below overrides
    url: https://api.openai.com/v1
    api_key: ${OPENAI_API_KEY}
    apis:
      - api: responses               # no `models:` → the catch-all for unmatched models
        default_model: gpt-5.5
      - api: image-generations
        models: ["gpt-image-*", "chatgpt-image-*"]
        default_model: gpt-image-2.5-flare
```

A **binding** is one entry in `apis:`:

| Key | Meaning |
|---|---|
| `api` | API definition name (required) |
| `models` | glob patterns of model ids this binding serves (optional; first binding without it is the **catch-all**) |
| `default_model` | the model used when a request asks for this binding's output kind without naming one |
| `url` | base-URL override (NVIDIA image models on `ai.api.nvidia.com`) |
| `auth`, `request`, `response` | deep-merged over the API definition, as provider entries are today |

Bindings are ordered: the first matching pattern wins, so specific rules go first.

### 3.2 Exclusions

Models a provider lists that no Lexi API can drive (yet):

```yaml
nvidia:
  unsupported:
    - { models: ["*guard*", "*safety*"], reason: "safety classifier" }
    - { models: ["*video-detector*"],   reason: "detector" }
```

They still show in `models hosted` (marked), and `respond` refuses them with the
reason instead of a provider 400. Adding a classifier API later = moving the
patterns into a binding.

### 3.3 The model catalog

Replaces `hosted_models` / `hosted_model_fields`, and adds a mapping from the
listing's fields to Lexi's capability descriptor:

```yaml
anthropic:
  catalog:
    path: /models                    # GET {url}{path}
    items_path: data                 # default: data
    id_path: id                      # default: id
    fields: [id, display_name, max_input_tokens, max_tokens]   # `models hosted` columns
    paginate: { after_param: after_id, has_more: has_more, last_id: last_id }
    capabilities:                    # listing entry → capability descriptor (§5)
      input:
        image: { path: capabilities.image_input.supported }
        document: { path: capabilities.pdf_input.supported }
      features:
        thinking: { path: capabilities.thinking.supported }
        tools: { const: true }
      limits:
        context_window: { path: max_input_tokens }
        max_output_tokens: { path: max_tokens }
      name: { path: display_name }
```

```yaml
nvidia:
  catalog:
    fields: [id, owned_by]
    capabilities:                    # read when present, ignored when absent
      input:
        image: { path: model_type, in: [vlm] }
      features:
        tools: { path: supports_tools }
        thinking: { path: supports_thinking }
      limits:
        context_window: { path: context_window }
```

A **value rule** is one of `{path}` (truthy), `{path, equals}`, `{path, in: […]}`,
`{const}`. A missing field yields "unknown", never "false" — unknown falls back
to the API definition's defaults. Rules are pure and live in `lexi-core`, pinned
by conformance cases (§9).

**Caching.** `models hosted` fetches and writes `providers/<p>/catalog.json`
(`{fetched_at, models}`); `--refresh` refetches. `respond` reads only the cache
— it never makes a hidden network call to classify a model.

### 3.4 Provider templates (shipped, like API definitions)

Shipped knowledge — bindings, patterns, catalog mappings, default models —
changes as providers change (`gpt-image-1` was shut down 2026-10-23). Today it
is seeded into the user's file once and never updated. Instead:

- Lexi embeds **provider templates** (`lexi-core/providers/openai.yaml`, …), the
  same way it embeds API definitions.
- A provider entry with `template: openai` is the template **deep-merged with
  the user's entry** (lists such as `apis` and `unsupported` merge by `api` /
  position — see open question Q3).
- Seeding writes `template:` entries, so a Lexi upgrade brings new patterns and
  models to existing installs; the user's own keys still win.
- A provider without `template:` is fully user-defined (any OpenAI-compatible
  server, a local model).

### 3.5 Backward compatibility

The current shape keeps working, read as:

| Current | Means |
|---|---|
| `api: X` + `default_model: M` | `apis: [{api: X, default_model: M}]` |
| `hosted_models: P` + `hosted_model_fields: F` | `catalog: {path: P, fields: F}` |
| `default-model.yaml` | the catch-all binding's default model |

No file is rewritten automatically. `lexi providers upgrade <name>` converts an
entry to the template form (showing the diff first).

## 4. API definition additions

Small and optional:

- `capabilities.output` is already there; it becomes **load-bearing** (selects a
  binding by output kind). Shipped defs are audited for it.
- `capabilities.features` — e.g. `[tools, thinking, streaming, background]`,
  so a model's features can narrow them.
- `response.artifacts` already exists (`image-generations`); add it to
  `responses` for the image-generation tool's output, so text **and** images come
  back from one call.
- New definitions as needed per provider (P2: `embeddings`, `nvidia-genai-image`).

## 5. Model resolution

One function in `lexi-host` (pure core in `lexi-core`):

```
resolve_model(provider, model?, output?) -> ResolvedModel {
    id, binding (api, url, merged config), capabilities, source
}
```

**Which binding** — first that applies:

1. **Configured model** with an explicit `api:` (`configured-models.yaml`) — the user's word wins.
2. **Binding patterns** (`apis[].models`), in order.
3. **Exclusions** (`unsupported`) → error with the reason.
4. **Catalog data** if a mapping derives an output kind (future-proofing; e.g. Gemini's `supportedGenerationMethods`).
5. **Catch-all** binding.

**Which model**, when none is given: the `default_model` of the binding whose
API produces the requested `output` kind (default `text`).

**Capabilities** = API definition's `capabilities` → refined by catalog data →
refined by `configured-models.yaml` `capabilities:` (engine:
`resolve_capabilities`, extended for `features`). `source` records which layer
decided, shown by `models hosted` and `--verbose`.

**Unconfigured models.** `--model` currently must be configured or aliased. With
classification, any id in the cached catalog is accepted; an id Lexi can't place
in a binding is refused with a hint (`models add <p> <id> --api …`).

### Worked examples

| Request | Resolves to |
|---|---|
| `respond openai "hi"` | catch-all `responses`, `gpt-5.5` |
| `respond openai "a fox" --model gpt-image-2.5-flare` | pattern → `image-generations` |
| `respond openai "a fox" --output image` | binding producing `image` → its default `gpt-image-2.5-flare` |
| `respond anthropic "describe" --image x.png --model claude-…` | `messages`; catalog says `image_input: true` → allowed |
| `respond nvidia "hi" --model nvidia/embed-qa-4` | pattern → `embeddings` binding (P2) — or, before P2, refused: "embedding model; not a chat model" |
| `respond nvidia "hi" --model nvidia/nemotron-3.5-content-safety` | exclusion → "safety classifier" |

## 6. One `respond` for every output

`respond` and `generate` merge into one pipeline in `lexi-host`:

1. **Resolve** provider + model + binding (§5).
2. **Validate** against capabilities before sending (input kinds via
   `validate_message`, requested output, features such as `--tool`/thinking,
   `max_tokens` ≤ limit). `--no-validate` skips it.
3. **Compose** through the binding's API definition. Schema flags come from
   *that* definition, so `--size`/`--quality`/`--n` appear for image models the
   way `--tool web_search` appears for Responses — no per-command flags.
4. **Send** (buffered / stream / background, as today).
5. **Extract** text (`text_extract`) **and** artifacts (`response.artifacts`).
6. **Save** artifacts under `--out` (default `.`) with non-colliding names
   (`<slug>-<n>.png`, never overwrite).

**Result** (`--json`, Python, desktop): `{provider, model, api, text,
artifacts: [{path, type, media_type, revised_prompt}], metadata, …}`. Streaming
adds an `artifact` event. Human output: text, then `Saved: <paths>`.

`generate` becomes an alias for `respond --output image` (deprecated for one
release, then removed). Python keeps `lexi.generate` as a convenience wrapper
over the same call.

## 7. Surfaces

| Surface | Change |
|---|---|
| CLI | `respond --output KIND`, `--out DIR`; `models hosted [--output KIND] [--refresh]` with `api`, `output`, `source` columns; `models add <p> <id> [--api A]` (inferred when omitted); `models default <p> [m] [--output KIND]`; `providers api add/rm`; `providers upgrade` |
| Python | `lexi.respond(..., output=, out_dir=)` returns `artifacts`; `lexi.models.hosted(p, output=, refresh=)`; `lexi.models.resolve(p, model=, output=)` |
| Desktop | Chat shows saved images inline; Models tab shows kind/API per model |
| Machine spec | `cli-machine-v0.md` → v1: `artifacts`, `api`, `artifact` stream event |

All behavior lives in `lexi-host`/`lexi-core`; the surface matrix and its CI
guard cover the new operations as usual.

## 8. Adding a provider (the extensibility test)

Adding Gemini should need **no Rust changes**:

1. **API definitions** for its protocols (`gemini-generate-content`, or reuse
   `chat-completions` against its OpenAI-compatible endpoint), with
   `capabilities` and `response.artifacts` for image output.
2. **A provider template** `gemini.yaml`: `url`, `auth`, bindings + patterns,
   `catalog` mapping (`supportedGenerationMethods` → output kind,
   `inputTokenLimit`/`outputTokenLimit` → limits).
3. **Conformance cases** for its classification and catalog mapping.

If a provider needs something config can't express, that's the signal to
extend the *config language* (a new value rule, a new binding key), not to add
provider-specific code.

## 9. Where the logic lives

- **`lexi-core`** (pure, conformance-pinned): glob matching, binding selection,
  exclusion checks, catalog value rules, capability merging. New conformance
  category `model/` with cases per provider template (OpenAI names, Anthropic
  catalog entries, NVIDIA's mixed list — using the real shapes observed).
- **`lexi-host`**: template loading + merging, catalog fetch/pagination/cache,
  `resolve_model`, the unified respond pipeline, artifact saving.
- **Surfaces**: flags and rendering only.

## 10. Delivery slices

| Slice | Delivers | Provider focus |
|---|---|---|
| **M1** | Config model: bindings, templates, back-compat reading; `resolve_model` (patterns + configured + catch-all); conformance `model/`; `models list` shows api/output | OpenAI, Anthropic |
| **M2** | Unified `respond` (text + artifacts, `--output`, `--out`, safe filenames, schema flags from the chosen API); `generate` alias; `responses` artifacts for the image tool; Python `artifacts` | OpenAI images |
| **M3** | Catalog: mapping rules, pagination, cache, `models hosted` columns/filter, unconfigured models; capability validation before send | Anthropic catalog |
| **M4** | NVIDIA: exclusions, `embeddings` API (`vector` output), per-binding URL, NVIDIA image API on its host | NVIDIA |
| **M5** | `providers api add/rm`, `providers upgrade`, desktop Models/Chat updates, machine spec v1 | all |

Each slice is its own PR with green CI; M1–M3 are P1.

## 11. Open questions

- **Q1 — `respond` positional grammar.** Today `respond openai hello` sends the
  saved prompt *named* `hello`. With one `respond` for everything, positional
  **text** reads better (`respond openai "a fox"`), saved prompts via
  `--prompt NAME`. Breaking, but early. *Recommend: switch in M2.*
- **Q2 — output vocabulary.** `text`, `image`, `audio`, `vector`, `video` — is
  `vector` the right name for embeddings, and should embeddings go through
  `respond` at all, or a separate `embed` command (different input shape: many
  strings, no conversation)? *Recommend: `embed` command, same resolution.*
- **Q3 — template list merging.** When a user edits `apis:` in a templated
  provider, does their list replace the template's, or merge by `api` name?
  *Recommend: merge by `api` name; `apis!:` (or `replace: true`) to replace.*
- **Q4 — catalog refresh.** Never automatic (only `models hosted`), or
  automatic when the cache is older than N days? *Recommend: never automatic.*
- **Q5 — model ids that change.** Template `default_model`s go stale
  (shutdowns). Use `shutdown_date` from OpenAI's catalog to warn in `respond`
  and `models hosted`? *Recommend: yes, in M3.*
