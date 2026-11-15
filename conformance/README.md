# Lexi Engine Conformance Suite

Language-neutral fixtures that pin the behavior of the Lexi API-definition engine
(see [`docs/spec/api-definition-v0.md`](../docs/spec/api-definition-v0.md)).

**Every engine implementation must pass these** — today's Python `api_compose`,
and the future Rust `lexi-core`. The suite is the oracle that lets us port the
engine to another language and *prove* identical behavior.

Fixtures are **JSON** (trivially parseable from any language). They reference API
definitions **by name** (`"api": "messages"`); each implementation resolves that
name to its own bundled copy of the definition under `apis/<name>.yaml`. The cases
are therefore pinned to the *current* shipped API definitions.

## Layout

```
conformance/
  cases/
    request/*.json     # request-body composition cases
    response/*.json    # response-extraction cases
  runners/             # (future) a thin runner per language
```

The Python runner lives at [`lexi-cli/tests/test_conformance.py`](../lexi-cli/tests/test_conformance.py)
and runs under the normal `pytest` invocation.

## Request case format

```jsonc
{
  "name": "messages-basic",
  "description": "...",
  "api": "messages",                 // resolved to apis/messages.yaml
  "compose": {
    "prompt_payload": { "prompt": "Hello", "role": "user", "model": "..." },
    "overrides": {},                 // optional
    "tool_presets": [],              // optional, applied in order
    "set_paths": [["a.b", 1]],       // optional, [path, value] pairs
    "unset_paths": []                // optional
  },
  "expect_body": { ... }             // exact composed body
}
```

## Response case format

```jsonc
{
  "name": "messages-text",
  "description": "...",
  "api": "messages",
  "response": { ... },               // a provider response object
  "expect": {
    "text": "...",                   // extract_text
    "activity": "...",               // summarize_activity (omit to skip)
    "annotations_rendered": "...",   // render_annotations(extract_annotations(...)) (omit to skip)
    "metadata": { ... }              // get_path per response.metadata map (omit to skip)
  }
}
```

Only the `expect` keys that are present are asserted (`text` is always asserted).
