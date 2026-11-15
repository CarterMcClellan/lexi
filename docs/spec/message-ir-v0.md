# Lexi Message IR Specification — v0

**Status:** v0 · 2026-11-14
**Audience:** engine implementers (Python `message_ir`, future Rust `lexi-core`).

A **message** is modeled as an ordered list of **typed content parts**, not a
bare string. This is the single most important forward-compatibility decision in
Lexi: by representing content as parts *now* — while only text exists — later
modalities (image, audio, video, tool calls) are **additive** rather than a
breaking rewrite.

This document is the authoritative contract for the IR, pinned by the
conformance fixtures under [`/conformance/cases/message`](../../conformance/README.md).
It complements the API-definition spec ([api-definition-v0.md](api-definition-v0.md)),
which describes how a *lowered* message is composed into a provider request body.

---

## 1. Data model

```
TextPart   = { type: "text", text: <string> }        # the only part type in v0
ContentPart = TextPart                                # a union, widened over time
Message    = { role: <string>, content: [ ContentPart, ... ] }
```

- `role` defaults to `"user"` when absent.
- `content` is an ordered list; order is significant and preserved.

Future part types (not in v0, shown for direction only):
`{ type: "image", source: {...} }`, `{ type: "audio", ... }`,
`{ type: "video", ... }`, `{ type: "tool_call", ... }`.

## 2. Parsing (`message_from_dict`)

`content` accepts three forms:

| Input `content` | Result |
|---|---|
| list of part dicts | each parsed via part rules below |
| a bare string | sugar for a single `TextPart` |
| absent / null | a single empty `TextPart` |

A part dict with `type: "text"` (or no `type`) becomes a `TextPart` with its
`text` (default `""`). **Any other `type` is an error in v0.**

## 3. Normalization from a legacy prompt payload (`normalize_prompt_payload`)

A legacy prompt payload is a dict `{ prompt: <string>, role: <string>, ... }`
(plus tuning fields like `temperature`, `max_tokens`). Normalization produces:

```
Message{ role: payload.role or "user", content: [ TextPart(payload.prompt or "") ] }
```

Tuning fields are **not** part of the message; they remain in the payload and are
handled by request composition, not the IR.

## 4. Lowering to template bindings (`to_prompt_bindings`)

The API-definition templates consume two bindings, `{{prompt}}` and `{{role}}`.
Lowering a message produces exactly those:

```
to_prompt_bindings(message) = {
  "role":   message.role,
  "prompt": concat(part.text for part in message.content),   # joiner ""
}
```

This is the **degenerate-case bridge**: for a single-text-part message it
reproduces precisely the inputs the engine used before the IR existed, so text
chat is behavior-identical (guaranteed by the conformance suite and by the full
existing test suite, since the respond path routes through the IR).

**Non-text parts are not representable in v0 lowering and raise.** That boundary
is the seam a later version extends: lowering will then emit provider-specific
content structures (e.g. Anthropic image blocks vs. OpenAI `image_url`),
selected by the API definition / capability registry.

## 5. Integration in the current engine

`respond_with_provider` normalizes the active prompt payload into a `Message`
and lowers it back before composing the request body:

```python
message = message_ir.normalize_prompt_payload(prompt_payload)
effective_payload = {**prompt_payload, **message_ir.to_prompt_bindings(message)}
```

For text chat `effective_payload == prompt_payload` (identity), so the IR is on
the real path without changing behavior.

## 6. Versioning

- v0 freezes the text-only contract above.
- Adding a content-part type, or changing lowering, is a version bump with
  new/updated conformance fixtures.
- See [target-architecture.md](../target-architecture.md) for the staged plan
  (vision input → image/video generation → tools) that extends this IR.

---

## v0.1 — Image input (Phase I)

### `ImagePart`

```
ImagePart = { type: "image", url: <string>, media_type?: <string> }
```

v0.1 supports **URL image sources** (base64 / Files-API uploads are a later
slice). `message_from_dict` parses `type: "image"` parts; `type` values other
than `text`/`image` still raise.

### Provider-aware lowering (`lower_message`)

`to_prompt_bindings` (text-only, provider-agnostic) stays as-is. Multimodal
content is lowered by **`lower_message(message, content_mapping)`**, which
returns `{role, content, prompt}`:

- `content` is the provider's per-message content — a **bare string** when the
  provider's `content_mapping.text_only_as_string` is set *and* the message is a
  single text part; otherwise a **list** of parts, each rendered through its
  type's template in `content_mapping.parts` (see the API-definition spec §9).
- `prompt` (the concatenated text) is retained so legacy `{{prompt}}` templates
  still work.
- When `content_mapping` is absent, lowering falls back to `{role, prompt}`.

Text chat is **byte-identical** to v0 (the shipped API-def templates now bind
`{{content}}`, and lowering reproduces the former inline content). Pinned by the
`conformance/cases/request/*-image-url.json` fixtures plus the unchanged text
fixtures.

### Image sources: URL and base64 (`by_source`)

An `ImagePart` carries **either** a `url` **or** inline base64 `data`
(+ `media_type`); `source_kind` is `"base64"` when `data` is present, else
`"url"`. Because providers render the two differently, the image entry in
`content_mapping.parts` uses a **`by_source`** map keyed by source kind instead
of a single flat `template`:

```yaml
image:
  by_source:
    url:    { template: { type: image, source: { type: url, url: "{{url}}" } } }
    base64: { template: { type: image, source: { type: base64, media_type: "{{media_type}}", data: "{{data}}" } } }
```

`lower_content_part` selects the sub-template by the part's `source_kind`, then
renders it with the part's variables: `{{url}}`, `{{data}}`, `{{media_type}}`,
and `{{data_uri}}` (a precomputed `data:<media_type>;base64,<data>` URI used by
providers that take base64 as a data URI — OpenAI Chat Completions / Responses).
The three shipped defs map base64 to: Anthropic → structured `base64` source;
Chat Completions / Responses → a `data:` URI in `image_url`. Pinned by the
`*-image-base64.json` request fixtures. URL output is unchanged.
