# Providers, APIs and Models — Design

**Status:** proposal for review · 2026-11-16 (rev 2: model tags)
**Scope:** how Lexi configures providers that offer several kinds of model
(text, image, embeddings, …) over several APIs, describes their models with
**tags**, and exposes one `respond` for every kind of output.
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

And providers describe their models very unevenly:

| Provider | `GET /models` per model | What Lexi can learn |
|---|---|---|
| OpenAI | `id, object, created, owned_by, shutdown_date` | names only (`gpt-image-*`, `text-embedding-*`, `tts-*`, …) — reliable |
| Anthropic | `id, display_name, created_at, max_input_tokens, max_tokens, capabilities{image_input, pdf_input, thinking, effort, structured_outputs, …}` | rich data; all models are Messages / text-out |
| NVIDIA | `id, object, created` (constant, meaningless), `owned_by`; *one* of 33 also had `model_type, context_window, supports_tools, supports_thinking` | names, loosely (`*embed*`, `*guard*`), plus partial data |
| Gemini (future) | `supportedGenerationMethods`, `inputTokenLimit`, `outputTokenLimit`, … | rich data |

**Requirements:**
1. Everything provider-specific lives in **configuration** (provider entries,
   API definitions). Code knows concepts, never provider names.
2. Where a provider can't tell Lexi what a model is, **the user can** — and the
   user's word wins.

## 2. Concepts

| Concept | Is | Defined in |
|---|---|---|
| **API definition** | One wire protocol: endpoint, request schema, response extraction, streaming, and what it carries | `lexi-core/apis/*.yaml` (shipped) / `apis/` (custom) — small additions (§6) |
| **Provider** | An account: identity, auth, base URL, the **APIs it offers** (bindings), its **model catalog**, and **tag rules** for its models | `providers-config.yaml`, over a shipped **provider template** (§4.4) |
| **Model** | An id the provider serves, described by a set of **tags** | tags come from tag rules, catalog data and the user (§3) |
| **Tag** | One predefined fact about a model: what it takes, what it produces, what it supports | the shipped **tag vocabulary** (§3.1) |

Tags are the single description of a model. Everything downstream reads them:
which API a model is sent to (§5), what `respond` validates before sending (§7),
what `models hosted --tag …` filters on.

## 3. Model tags

### 3.1 The vocabulary (predefined)

Shipped with Lexi (`lexi-core/tags.yaml`), namespaced, closed: a tag not in the
vocabulary is rejected, so typos can't silently change routing.

| Namespace | Tags | Meaning |
|---|---|---|
| `in:` | `text`, `image`, `audio`, `video`, `document` | accepted input |
| `out:` | `text`, `image`, `audio`, `video`, `vector` | produced output (`vector` = embeddings) |
| `kind:` | `chat`, `embedding`, `image-generation`, `speech`, `transcription`, `rerank`, `classifier`, `detector` | what the model is for (one per model) |
| `feature:` | `tools`, `thinking`, `structured-output`, `web-search`, `streaming`, `background`, `files` | optional abilities |
| `status:` | `preview`, `deprecated` | lifecycle |

Each `kind:` implies default `in:`/`out:` tags (`kind:image-generation` →
`in:text`, `out:image`), so a rule usually needs only one tag. New tags are a
vocabulary change (a PR), which keeps the set meaningful across providers.

**Labels** (open question Q6): free-form user tags (`label:fast`, `label:cheap`)
for filtering only — they never affect routing or validation.

### 3.2 Where tags come from

Four sources, merged in this order (later wins):

| # | Source | Example | Written by |
|---|---|---|---|
| 1 | **API defaults** — the binding's API definition | `messages` → `in:text in:image out:text kind:chat` | shipped |
| 2 | **Tag rules** — provider config, by model-id pattern | `"gpt-image-*": [kind:image-generation]` | shipped template (+ user) |
| 3 | **Catalog data** — the provider's model listing, via mapping rules | Anthropic `capabilities.thinking.supported: true` → `feature:thinking` | automatic |
| 4 | **User tags** — explicit, per model or pattern | `+out:image`, `-in:image` | the user (`models tag`) |

A source can **add** a tag or **remove** one (`-in:image`). Missing data never
removes anything: if a listing omits a field, the tag simply isn't derived.
Every effective tag remembers its source, so `models tags` can say *why*.

### 3.3 Tag rules (provider config)

```yaml
openai:
  tag_rules:                          # first matching rule per tag namespace wins
    "gpt-image-*":        [kind:image-generation]
    "chatgpt-image-*":    [kind:image-generation]
    "text-embedding-*":   [kind:embedding]
    "tts-*":              [kind:speech]
    "whisper-*":          [kind:transcription]
    "omni-moderation-*":  [kind:classifier]
    "gpt-*":              [feature:tools, feature:web-search]

nvidia:
  tag_rules:
    "*embed*":            [kind:embedding]
    "nvidia/nvclip":      [kind:embedding, in:image]
    "*guard*":            [kind:classifier]
    "*safety*":           [kind:classifier]
    "*video-detector*":   [kind:detector]
    "*-vl-*":             [in:image]
```

### 3.4 User tags

Stored per provider in `providers/<p>/model-tags.yaml` — separate from
`configured-models.yaml`, so you can tag any hosted model without configuring it.
Keys are model ids or patterns:

```yaml
nvidia/nemotron-3.5-super-vl-120b-a12b: [+feature:tools, +feature:thinking]
some-vendor/new-image-model:           [+kind:image-generation]
"meta/*":                              [-feature:tools]
```

```bash
lexi models tag nvidia some-vendor/new-image-model +kind:image-generation
lexi models tag nvidia "meta/*" -feature:tools
lexi models tags nvidia some-vendor/new-image-model     # effective tags + where each came from
lexi models hosted openai --tag out:image               # filter
lexi tags                                                # the vocabulary
```

## 4. Provider configuration

### 4.1 Bindings: the APIs a provider offers, chosen by tags

```yaml
providers:
  openai:
    template: openai                 # shipped defaults (§4.4); everything below overrides
    url: https://api.openai.com/v1
    api_key: ${OPENAI_API_KEY}
    apis:
      - api: responses               # no `for:` → the catch-all
        default_model: gpt-5.5
      - api: image-generations
        for: [kind:image-generation]
        default_model: gpt-image-2.5-flare
```

| Binding key | Meaning |
|---|---|
| `api` | API definition name (required) |
| `for` | tags a model must have to use this binding (all of them); omitted = catch-all |
| `default_model` | the model used when a request asks for this binding's output without naming one |
| `url` | base-URL override (NVIDIA image models on `ai.api.nvidia.com`) |
| `auth`, `request`, `response` | deep-merged over the API definition, as provider entries are today |

Bindings are ordered; the first whose `for:` tags a model has wins. The catch-all
only takes models whose `kind:` it can serve (its API's `kind:` default), so an
embedding model never falls through to chat.

**No binding = unsupported.** A model tagged `kind:classifier` on a provider with
no classifier binding is listed by `models hosted` (marked) and refused by
`respond` with the reason ("nvidia/nemotron-3.5-content-safety is a classifier;
no API configured for it") instead of a provider 400. Supporting it later is a
new binding, not a code change.

### 4.2 The model catalog

Replaces `hosted_models` / `hosted_model_fields`; its `tags` section maps listing
fields to tags (source 3):

```yaml
anthropic:
  catalog:
    path: /models
    items_path: data                 # default: data
    id_path: id                      # default: id
    fields: [id, display_name, max_input_tokens, max_tokens]   # `models hosted` columns
    paginate: { after_param: after_id, has_more: has_more, last_id: last_id }
    tags:
      in:image:               { path: capabilities.image_input.supported }
      in:document:            { path: capabilities.pdf_input.supported }
      feature:thinking:       { path: capabilities.thinking.supported }
      feature:structured-output: { path: capabilities.structured_outputs.supported }
    limits:
      context_window:    { path: max_input_tokens }
      max_output_tokens: { path: max_tokens }
    name: { path: display_name }
```

```yaml
nvidia:
  catalog:
    fields: [id, owned_by]
    tags:
      in:image:         { path: model_type, in: [vlm] }
      feature:tools:    { path: supports_tools }
      feature:thinking: { path: supports_thinking }
    limits:
      context_window: { path: context_window }
```

A **value rule** is `{path}` (truthy), `{path, equals}`, `{path, in: […]}` or
`{const}`. True adds the tag, false removes it, missing does nothing. Rules are
pure, in `lexi-core`, pinned by conformance cases (§9). `limits` and `name` feed
the model's limits and display name the same way.

**Caching.** `models hosted` fetches and writes `providers/<p>/catalog.json`
(`{fetched_at, models}`); `--refresh` refetches. `respond` reads only the cache,
never making a hidden network call to classify a model.

### 4.3 Configured models

`configured-models.yaml` keeps names, descriptions, limits and aliases, and may
pin a binding outright (`api: image-generations`) — the strongest signal, above
tags, for the odd model that fits no rule.

### 4.4 Provider templates (shipped, like API definitions)

Bindings, tag rules, catalog mappings and default models change as providers
change (`gpt-image-1` was shut down 2026-10-23), but today they're seeded into
the user's file once and never updated. Instead:

- Lexi embeds **provider templates** (`lexi-core/providers/openai.yaml`, …), as it
  embeds API definitions.
- An entry with `template: openai` is the template **deep-merged with the user's
  entry**; `apis` merge by `api` name and `tag_rules` by pattern (Q3).
- Seeding writes `template:` entries, so a Lexi upgrade brings new rules and models
  to existing installs. User tags (`model-tags.yaml`) are never touched.
- A provider without `template:` is fully user-defined (any OpenAI-compatible
  server, a local model). With no tag rules and no catalog data, its models get
  their binding's API defaults, and the user tags the rest.

### 4.5 Backward compatibility

| Current | Means |
|---|---|
| `api: X` + `default_model: M` | `apis: [{api: X, default_model: M}]` |
| `hosted_models: P` + `hosted_model_fields: F` | `catalog: {path: P, fields: F}` |
| `default-model.yaml` | the catch-all binding's default model |

No file is rewritten automatically; `lexi providers upgrade <name>` converts an
entry to the template form, showing the diff first.

## 5. Model resolution

```
resolve_model(provider, model?, output?) -> ResolvedModel {
    id, tags (with sources), binding (api, url, merged config), limits
}
```

1. **Tags**: API defaults → tag rules → catalog data → user tags (§3.2).
2. **Binding**: a configured model's pinned `api:`, else the first binding whose
   `for:` tags the model has, else the catch-all if it can serve the model's
   `kind:`, else **unsupported** (error with the reason).
3. **No model given**: the `default_model` of the binding whose API produces the
   requested `output` (default `out:text`).
4. **Unconfigured models**: any id in the cached catalog (or matching a tag rule)
   is accepted; one Lexi can't place is refused with a hint
   (`models tag <p> <id> +kind:…`).

| Request | Resolves to |
|---|---|
| `respond openai "hi"` | catch-all `responses`, `gpt-5.5` |
| `respond openai "a fox" --model gpt-image-2.5-flare` | rule → `kind:image-generation` → `image-generations` |
| `respond openai "a fox" --output image` | binding producing `out:image` → `gpt-image-2.5-flare` |
| `respond anthropic "describe" --image x.png` | catalog → `in:image` → allowed |
| `respond nvidia "hi" --model nvidia/embed-qa-4` | rule → `kind:embedding` → `embeddings` binding (P2), or refused before P2 |
| `respond nvidia "hi" --model nvidia/nemotron-3.5-content-safety` | `kind:classifier`, no binding → refused with reason |
| `respond nvidia --image x.png --model some/new-vlm` | no `in:image` → refused; after `models tag nvidia some/new-vlm +in:image` → allowed |

All of this is pure logic in `lexi-core` over the provider config, the catalog
cache and the user tags.

## 6. API definition additions

- `capabilities` gains `kind` and `features`, and is read as the binding's
  **default tags** (`input: [text, image]` → `in:text in:image`).
- `response.artifacts` (already in `image-generations`) is added to `responses`
  for the image-generation tool, so text **and** images come back from one call.
- New definitions as needed (P2: `embeddings`, `nvidia-genai-image`).

## 7. One `respond` for every output

`respond` and `generate` merge into one pipeline in `lexi-host`:

1. **Resolve** provider, model, binding (§5).
2. **Validate** against the model's tags before sending: `--image` needs
   `in:image`, `--tool` needs `feature:tools`, a thinking preset needs
   `feature:thinking`, `max_tokens` ≤ the limit. `--no-validate` skips it.
3. **Compose** through the binding's API definition. Schema flags come from that
   definition, so `--size`/`--quality`/`--n` appear for image models the way
   `--tool web_search` appears for Responses.
4. **Send** (buffered / stream / background).
5. **Extract** text and artifacts.
6. **Save** artifacts under `--out` (default `.`) with non-colliding names.

Result (`--json`, Python, desktop): `{provider, model, api, tags, text,
artifacts: [{path, type, media_type, revised_prompt}], metadata, …}`; streaming
adds an `artifact` event. `generate` becomes an alias for `respond --output image`
(deprecated for one release). Python keeps `lexi.generate` as a wrapper.

## 8. Surfaces

| Surface | Change |
|---|---|
| CLI | `respond --output KIND --out DIR`; `models hosted [--tag T]… [--refresh]` with `kind`, `api`, tag columns; `models tag <p> <model\|pattern> (+T\|-T)…`; `models tags <p> <model>`; `tags`; `models default <p> [m] --output KIND`; `providers api add/rm`; `providers upgrade` |
| Python | `lexi.respond(..., output=, out_dir=)` returns `artifacts`; `lexi.models.hosted(p, tags=, refresh=)`; `lexi.models.tag(p, model, add=, remove=)`; `lexi.models.tags(p, model)`; `lexi.models.resolve(p, model=, output=)`; `lexi.tags()` |
| Desktop | Chat shows saved images inline; Models tab shows tags (with source) and lets you add/remove them |
| Machine spec | `cli-machine-v0.md` → v1: `artifacts`, `api`, `tags`, `artifact` stream event |

## 9. Where the logic lives

- **`lexi-core`** (pure, conformance-pinned): tag vocabulary, glob matching, tag
  merging with sources, catalog value rules, binding selection. New conformance
  category `model/`, with cases built from the real listing shapes (OpenAI
  names, Anthropic catalog entries, NVIDIA's mixed list).
- **`lexi-host`**: template loading and merging, catalog fetch, pagination and
  cache, `model-tags.yaml` read/write, `resolve_model`, the unified respond
  pipeline, artifact saving.
- **Surfaces**: flags and rendering only.

## 10. Adding a provider (the extensibility test)

Adding Gemini should need **no Rust changes**:

1. **API definitions** for its protocols (native `generateContent`, or
   `chat-completions` against its OpenAI-compatible endpoint), with
   `capabilities` and `response.artifacts`.
2. **A provider template**: `url`, `auth`, bindings, tag rules, and a catalog
   mapping (`supportedGenerationMethods` → `kind:` tags,
   `inputTokenLimit`/`outputTokenLimit` → limits).
3. **Conformance cases** for its tag derivation.

Anything a template can't express is a reason to extend the config language (a
new value rule, a new tag), not to write provider code. And whatever a template
gets wrong, a user can fix with tags.

## 11. Delivery slices

| Slice | Delivers | Provider focus |
|---|---|---|
| **M1** | Tag vocabulary; tag rules; user tags (`models tag`/`tags`, `lexi tags`); bindings chosen by tags; templates; back-compat; `resolve_model`; conformance `model/` | OpenAI, Anthropic |
| **M2** | Unified `respond` (text + artifacts, `--output`, `--out`, safe filenames, schema flags from the chosen API); `generate` alias; Python `artifacts` | OpenAI images |
| **M3** | Catalog: tag/limit mapping rules, pagination, cache, `models hosted --tag`, unconfigured models; validation before send | Anthropic catalog |
| **M4** | NVIDIA: tag rules, unsupported kinds, `embeddings` API (`out:vector`), per-binding URL, image API on its host | NVIDIA |
| **M5** | `providers api add/rm`, `providers upgrade`, desktop Models/Chat, machine spec v1 | all |

Each slice is its own PR with green CI; M1–M3 are P1.

## 12. Open questions

- **Q1 — `respond` grammar.** Positional **text** (`respond openai "a fox"`), with
  saved prompts via `--prompt NAME`? Breaking, but early. *Recommend: yes, in M2.*
- **Q2 — embeddings.** Through `respond`, or a separate `embed` command (many
  strings in, vectors out, no conversation)? *Recommend: `embed`, same resolution.*
- **Q3 — template merging.** User `apis:` / `tag_rules:` merge with the
  template's by key, or replace them? *Recommend: merge; `replace: true` to replace.*
- **Q4 — catalog refresh.** Only on `models hosted`, or automatically when stale?
  *Recommend: only on `models hosted`.*
- **Q5 — shutdowns.** Turn OpenAI's `shutdown_date` into `status:deprecated` and
  warn in `respond`? *Recommend: yes, in M3.*
- **Q6 — free-form labels.** Allow user `label:*` tags (filter-only, never
  routing)? *Recommend: yes — the closed vocabulary keeps routing safe, labels
  give users their own grouping.*
- **Q7 — tag scope.** User tags per provider (`nvidia/foo` on NVIDIA only), or
  also global patterns across providers? *Recommend: per provider; a global
  `model-tags.yaml` can come later if needed.*
