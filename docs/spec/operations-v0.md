# Lexi Async Operation Specification — v0

**Status:** v0 · 2026-11-15
**Audience:** engine implementers (Python `operations`, Rust `lexi-core::operations`).

Some provider APIs are **asynchronous**: a submit returns an operation handle and
an initial status; the caller **polls** until a terminal state, then reads the
result from the final response. This document specifies the `operation` block an
API def uses to describe that lifecycle, and the pure engine that interprets it.

The HTTP polling loop lives in the **host** (CLI/GUI); the engine does no I/O —
it only reads/classifies the handle and builds the poll path. Pinned by
`conformance/cases/operation/`.

---

## 1. Canonical states

```
pending · running · succeeded · failed · unknown
```

`succeeded` and `failed` are **terminal** (`done = true`). `unknown` is the
classification for an absent or unmapped status.

## 2. `operation` block

```yaml
operation:
  id_path: id                         # path to the operation id in a response
  status_path: status                 # path to the raw status value
  poll:
    method: GET
    path_template: "/responses/{id}"  # {id} substituted with the operation id
  status_map:                         # raw provider status -> canonical state
    pending:   [queued]
    running:   [in_progress]
    succeeded: [completed]
    failed:    [failed, cancelled, incomplete]
```

## 3. Engine

- `classify_status(status, status_map) -> state` — the first canonical state (in
  the order `pending, running, succeeded, failed`) whose list contains the raw
  status; `unknown` if none match or the status is null/absent.
- `read_operation(response, operation) -> { id, status, state, done }` — reads
  `id_path`/`status_path`, classifies, and marks `done` when terminal. Works on
  both the submit response and each poll response. With no `operation` block:
  `{ id: null, status: null, state: "unknown", done: false }`.
- `poll_path(operation, id) -> string` — `path_template` with `{id}` replaced
  (null id → empty string).
- `is_terminal(state) -> bool`.

## 4. Lifecycle (host)

```
submit:  compose body (e.g. background: true) -> POST -> op = read_operation(submit_response)
poll:    while not op.done:  sleep; GET base_url + poll_path(operation, op.id); op = read_operation(poll_response)
result:  if op.state == "succeeded":  extract_text / extract_artifacts on the final poll response
```

The result is read with the **same** `response.text_extract` / `response.artifacts`
config as a synchronous call — async changes only *when* the result is available,
not *how* it is extracted.

## 5. Versioning

- The shipped `responses` def carries an `operation` block for OpenAI background
  mode. The same shape generalizes to video generation and other job-style APIs
  (submit → poll → download), which is the staged driver
  (see [target-architecture.md](../target-architecture.md)).
- Adding fields (e.g. a result URL path for download-style APIs, retry/backoff
  hints, webhook support) or changing classification is a version bump with
  new/updated conformance fixtures.
