# Lexi Files-API (Uploads) Specification — v0

**Status:** v0 · 2026-11-15
**Audience:** engine implementers (Python `uploads`, Rust `lexi-core::uploads`).

Some providers take large inputs by **uploading a file first**, then referencing
the returned handle. This has two halves:

1. **Upload** — POST the file (multipart) and read back a **file id**.
2. **Reference** — use that id as an image content part's `file_id` source
   (see [message-ir-v0.md](message-ir-v0.md) — `source_kind = "file"`).

The multipart HTTP upload is host-side; the engine only **interprets** the API
def's `files` block (where to POST, how to read the id). Pinned by
`conformance/cases/upload/` and the `*-image-file.json` request fixtures.

---

## 1. `files` block

```yaml
files:
  upload:
    method: POST
    path: /files          # upload endpoint
    purpose_field: purpose  # optional multipart field name for the upload purpose
  id_path: id             # where the file id is in the upload response
```

## 2. Engine

- `upload_path(files) -> string` — `files.upload.path` (or `""`).
- `upload_purpose_field(files) -> string?` — `files.upload.purpose_field`, if any.
- `read_upload(upload_response, files) -> { id }` — the file id at `files.id_path`
  (default `id`), or `null`. With no `files` block: `{ id: null }`.

## 3. Reference side (`source_kind = "file"`)

An `ImagePart` with a `file_id` lowers via the image entry's `by_source.file`
template:

- Anthropic → `{ type: image, source: { type: file, file_id: "{{file_id}}" } }`
- Responses → `{ type: input_image, file_id: "{{file_id}}" }`

(Chat Completions has no file image source and so declares no `by_source.file`.)

`source_kind` precedence is **file > base64 > url**, so a part carrying a
`file_id` renders as a file reference.

## 4. Lifecycle (host)

```
upload:    POST upload_path(files) with the file bytes (multipart) -> { id } = read_upload(resp, files)
reference: build an ImagePart { file_id: id } and compose as usual
```

## 5. Versioning

- v0 covers the id handle + path. Future: richer handles (e.g. Gemini's
  `file_uri` + `mime_type`), expiry, and non-image file references (documents,
  audio), plus capability gating of which sources a model accepts.
