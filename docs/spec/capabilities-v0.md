# Lexi Capability Registry Specification — v0

**Status:** v0 · 2026-11-15
**Audience:** engine implementers (Python `capabilities`, Rust `lexi-core::capabilities`).

A **capability descriptor** declares what a provider/model can accept and
produce, so portable code can route, validate, and fail with a clear message
instead of an opaque provider 4xx. Pinned by `conformance/cases/capability/`.

---

## 1. Descriptor

```
Capabilities = {
  input:  [ <modality>, ... ],     # modalities accepted as input  (e.g. "text", "image")
  output: [ <modality>, ... ],     # modalities produced as output
  limits: { <name>: <value>, ... } # optional, free-form (context window, max images, ...)
}
```

Modalities are the content-part `type` strings from the message IR (`text`,
`image`, …). v0 ships `input: [text, image]`, `output: [text]` for all three API
families.

## 2. Declaration

An API definition declares the **family baseline** under `capabilities`:

```yaml
capabilities:
  input: [text, image]
  output: [text]
```

## 3. Resolution (`resolve_capabilities`)

```
resolve_capabilities(api_def, model_capabilities?) -> Capabilities
```

- Start from `api_def.capabilities` (missing → empty `input`/`output`/`limits`).
- A **per-model override** (`model_capabilities`), when present:
  - replaces `input` **wholesale** if the key is present (a present-but-null value → empty list),
  - replaces `output` **wholesale** likewise,
  - **merges** `limits` key-by-key over the baseline.

The result always has all three keys (`input`, `output`, `limits`).

> Per-model capabilities are resolved from whatever override the caller supplies
> (e.g. a model's entry in `configured-models.yaml`); wiring that lookup into the
> CLI/GUI is downstream of this engine contract.

## 4. Queries & validation

- `supports_input(cap, modality) -> bool` — membership in `cap.input`.
- `supports_output(cap, modality) -> bool` — membership in `cap.output`.
- `validate_message(cap, message) -> [error, ...]` — for each content part whose
  modality is **not** in `cap.input`, one error
  `"model does not accept <modality> input"`, **deduplicated per modality** in
  first-seen order. Empty list = valid.

The error string is part of the contract (both engines must match it).

## 5. Versioning

- Adding a modality, a limit key, or changing resolution/validation is a version
  bump with new/updated conformance fixtures.
- Future: richer limits (max image count/size, context window, pricing), output
  validation for generation APIs, and capability-based routing/fallback across
  providers (see [target-architecture.md](../target-architecture.md)).
