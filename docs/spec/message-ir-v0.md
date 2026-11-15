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
