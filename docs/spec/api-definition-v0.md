# Lexi API-Definition Specification — v0

**Status:** v0 (frozen snapshot of current behavior) · 2026-11-14
**Audience:** implementers of the Lexi engine (current Python `api_compose`, future Rust `lexi-core`) and authors of API definitions.

An **API definition** describes how to talk to one provider API family (e.g. Anthropic Messages, OpenAI Chat Completions, OpenAI Responses) as **data**: how to authenticate, how to build the request body, and how to extract results from the response. The engine that interprets a definition is small and pure (dict in → dict out, no I/O).

This document is the authoritative description of the format. It is the contract that every engine implementation must honor, pinned by the language-neutral conformance suite in [`/conformance`](../../conformance/README.md).

> **Scope of v0.** This freezes *today's* text-chat behavior exactly. Multimodal content parts, async operations, and the capability registry (see [target-architecture.md](../target-architecture.md)) extend this format in later versions; v0 is the baseline the Rust port must reproduce byte-for-byte.

---

## 1. Top-level structure

```yaml
name: <string>              # required, non-empty — the API's identifier
description: <string>       # optional, human-readable
auth:      { ... }          # optional — how to authenticate (see §2)
request:   { ... }          # required — method, path, body_schema (see §3)
response:  { ... }          # required — how to read the response (see §5)
tool_presets: { ... }       # optional — named body fragments (see §4)
```

A provider references an API definition by name and may inline-override any of the `auth` / `request` / `response` / `tool_presets` blocks (see §7).

---

## 2. `auth` block

```yaml
auth:
  header: x-api-key            # default: "Authorization"
  value_prefix: ""             # default: "Bearer "
  extra_headers:               # optional, name -> value (stringified)
    anthropic-version: "2023-06-01"
```

Header construction (given an `api_key`):

1. Always set `Content-Type: application/json` and `Accept: application/json`.
2. Set `<header>: <value_prefix><api_key>`.
3. Add every `extra_headers` entry, values coerced to string.

Defaults reproduce Bearer-token auth when `auth` is absent.

---

## 3. `request` block

```yaml
request:
  method: POST                 # informational in v0 (always POST today)
  path: /messages              # required, non-empty — appended to provider url
  body_schema: { ... }         # required — a JSON Schema (object) + x-extensions
```

### 3.1 `body_schema`

`body_schema.type` must be `object` (or omitted). Its `properties` map describes each request-body field with standard JSON-Schema keywords **plus** Lexi `x-*` extensions. Property **declaration order is significant** — the composed body emits keys in `properties` order.

#### Supported JSON-Schema keywords (validation subset)

The engine validates the composed body against `body_schema` using this subset:

| Keyword | Behavior |
|---|---|
| `type` | one of `string, integer, number, boolean, array, object, null`, or a list of these. `number` accepts int or float; `boolean` is **not** a number. |
| `const` | value must equal exactly. |
| `enum` | value must be one of. |
| `minimum` / `maximum` | numeric bounds (ignored for booleans). |
| `required` | listed keys must be present on an object. |
| `properties` | recurse into known child properties (unknown keys are allowed). |
| `items` | recurse into every array element against one schema. |
| `oneOf` | must match **exactly one** alternative (by successful sub-validation). |

Anything else is ignored (lenient). Unknown `type` names validate as true.

#### `x-*` extensions

- **`x-bind: { from: <var> }`** — the value comes from binding variable `<var>` (defaults to the property name). Bindings are `prompt_payload` merged with non-null `overrides` (see §3.2). If the binding is absent/null, fall back to `default` if present, else omit.
- **`x-render: { template: <any> }`** — the value is the `template` with `{{var}}` placeholders substituted from bindings (see §3.3). Always considered present.
- **`x-include: <mode>`** — controls whether a resolved value is emitted. See §3.4.
- **`x-cli: { flag, alias, repeat, invert, enum_map, description }`** — exposes the property as a CLI flag. Consumed by CLI front-ends (flag introspection), **not** by body composition. The composer ignores it.

### 3.2 Composition algorithm

Inputs: the `api_def`, a `prompt_payload` dict, an `overrides` dict, a list of selected `tool_presets`, a list of `set_paths` (`(path, value)`), a list of `unset_paths`.

```
bindings = prompt_payload  overlaid with  { k: v for k,v in overrides if v is not None }

body = {}
for (prop_name, prop_schema) in body_schema.properties:   # declaration order
    (value, present) = resolve_property(prop_name, prop_schema, prompt_payload, overrides, bindings)
    if not present: continue
    mode = prop_schema["x-include"] or default_include(prop_schema)
    if not should_include(value, mode): continue
    body[prop_name] = value

for preset in tool_presets_selected:        # KeyError if unknown
    deep_merge_concat(body, tool_presets[preset])

for (path, value) in set_paths:   set_path(body, path, value)
for path in unset_paths:          unset_path(body, path)

if not skip_validation:
    errors = validate(body_schema, body); raise if errors
return body
```

**Precedence (low → high):** schema `default` → `prompt_payload` → `overrides` → tool presets → `set_paths` → `unset_paths`.

`resolve_property`:
1. if `x-bind`: use `bindings[var]` if present and non-null; else `default` if present; else omit.
2. if `x-render`: render the template (always present).
3. else if `prop_name` in `overrides` (non-null): use it.
4. else if `prop_name` in `prompt_payload` (non-null): use it.
5. else if `default` present: use it.
6. else omit.

### 3.3 Template rendering (`x-render`, and `{{var}}` anywhere)

- A string that is **exactly** `"{{var}}"` (after trimming) is replaced with the **raw** binding value — so an int/list/dict survives with its type intact. If `var` is not a binding, the literal is left unchanged.
- A placeholder **embedded** in a larger string is substituted with the **stringified** value; unknown placeholders are left verbatim.
- Dicts and lists are rendered recursively; other scalars pass through unchanged.

### 3.4 `x-include` modes

The default when `x-include` is omitted: **`always`** if the property has a `default`, `x-bind`, or `x-render`; otherwise **`if_set`**.

| Mode | Emit the field when… |
|---|---|
| `always` | always |
| `if_set` | value is not null |
| `if_truthy` | value is truthy |
| `if_non_empty` | value is non-null and (has `len > 0`, or is not a sized type) |
| `if_any_child_set` | dict with any non-null value / non-empty list / non-null scalar |

An invalid `x-include` mode is an error.

### 3.5 Path expressions (`set_paths`, `unset_paths`, response extraction)

A path is dotted with bracketed integer indices: `"reasoning.effort"`, `"choices[0].message.content"`. Parsing yields segments (`str` keys, `int` indices). `get_path` returns `null` on any miss. `set_path` creates intermediate dicts/lists as needed. `unset_path` is a no-op if absent.

---

## 4. `tool_presets`

```yaml
tool_presets:
  thinking_low: { thinking: { type: enabled, budget_tokens: 4096 } }
  web_search:   { tools: [ { type: web_search } ] }
```

A map of preset name → body fragment. Selected presets are **deep-merged into the body in selection order**, after property resolution and before `set_paths`. Deep-merge semantics here (**preset merge**):

- nested dicts merge recursively,
- **lists are concatenated**,
- scalars overwrite.

Selecting an unknown preset is an error.

> ⚠️ This list-**concatenation** differs from the provider-override merge in §7, which **replaces** lists. Two distinct merge rules; both are part of the contract.

---

## 5. `response` block

```yaml
response:
  text_extract: { ... }   # required — how to pull the assistant text (§5.1)
  activity:     { ... }   # optional — one-line tool/activity summary (§5.2)
  annotations:  { ... }   # optional — citations (§5.3)
  metadata:     { ... }   # optional — field -> path map (§5.4)
  stream:       { ... }   # optional — SSE delta/done config (§5.5)
```

### 5.1 `text_extract` — two modes (exactly one)

**Mode A — simple path:**
```yaml
text_extract: { text_path: "choices[0].message.content" }
```
Return the string at `text_path`, or `""` if missing / not a string.

**Mode B — walk and filter:**
```yaml
text_extract:
  items_path: output          # list to walk
  item_filter: { type: message }   # keep items matching all k/v (optional)
  content_path: content       # sub-list per item (optional; else the item itself)
  content_filter: { type: output_text }  # keep content nodes matching all k/v (optional)
  text_field: text            # field to read from each node (default "text")
  joiner: ""                  # concatenation separator (default "")
```
Walk `items_path`; for each item passing `item_filter`, take `content_path` (or the item), normalize to a list, and for each node passing `content_filter`, append `node[text_field]` if it is a string. Join with `joiner`. Returns `""` when nothing matches.

A filter matches an item iff every key/value in it equals the item's corresponding field. An empty filter matches everything; a non-dict item matches only an empty filter.

Specifying both `text_path` and `items_path`, or neither, is invalid.

### 5.2 `activity`

```yaml
activity:
  items_path: output                      # default "output"
  summarize_by_type:                       # emit order = declaration order
    web_search_call: "{count} web search"
    reasoning: "reasoning"
```
Count items in `items_path` by their `type`. For each `summarize_by_type` entry with a non-zero count, format the label (`{count}` substituted; labels without `{count}` are static). Join non-empty parts with `" · "`.

### 5.3 `annotations`

```yaml
annotations:
  scope: text_items            # "text_items" (default) or "response_root"
  path: annotations            # where annotations live, relative to each source
  types:
    url_citation: { format: "[{i}] {url} — {title}" }
```
- `scope: text_items` walks the Mode-B content nodes (the same nodes `text_extract` matched); `scope: response_root` walks the response object itself.
- From each source, read the list at `path`; keep annotations whose `type` is a key in `types`, in document order.
- **Render:** each annotation uses its type's `format`; `{i}` is auto-numbered from 1; other `{name}` placeholders come from the annotation's fields; missing fields render as empty strings. Lines joined with `\n`.

### 5.4 `metadata`

```yaml
metadata:
  model: model
  input_tokens: usage.input_tokens
  stop_reason: choices[0].finish_reason
```
A map of output name → path expression; each value is `get_path(response, path)`.

### 5.5 `stream`

```yaml
stream:
  delta_event: content_block_delta     # SSE event name carrying deltas (null = unnamed events)
  delta_path: delta.text               # path within an event's data to the text fragment
  done_event: message_stop             # event that terminates the stream
  done_marker: "[DONE]"                # literal data sentinel (Chat Completions)
  final_event: response.completed      # event whose data holds the full final object (optional)
  activity_events:                     # event name -> status line (optional)
    response.web_search_call.searching: "🔍 searching the web"
```
Interpreted by streaming consumers. `activity_events`, if present, must map event names to string labels.

**Classification (v0.1).** The host splits the SSE byte stream into event blocks;
`parse_sse_block` reads each into `{ event, data }` (one optional leading space
stripped from `data:` lines, multi-`data:` joined with `\n`). `classify_event`
then maps an `(event, data)` pair to a typed outcome, in this order:

1. `done` — `data` equals `done_marker` (e.g. Chat Completions `[DONE]`), **or**
   `event` equals `done_event`.
2. `activity` — `event` is a key in `activity_events` (→ its label).
3. `delta` — `delta_event` matches (a string event name, or null/absent = any)
   **and** `delta_path` resolves to a string in the parsed `data` (→ the fragment).
4. `other` — none of the above.

Pinned by `conformance/cases/stream/` across the Rust core and the wheel.

---

## 6. Validation (`validate_api_definition`)

An API definition is rejected if:
- `name` is missing/empty, or `request` / `response` blocks are missing;
- `request.path` is missing/empty, or `request.body_schema` is not an object (`type` other than `object`), or a property is not a dict, or an `x-include` mode is invalid;
- any **legacy** key appears: `request.{body,parameters,param_mapping,tools,tools_key}` or `response.{text_path,text_fallbacks}` (these moved to `body_schema` / `text_extract`);
- `response.text_extract` is missing, or specifies both/neither of `text_path`/`items_path`, or `item_filter`/`content_filter` is a non-dict;
- `tool_presets`, `response.metadata`, `response.stream`, or `stream.activity_events` are present but of the wrong type.

---

## 7. Provider config & effective-config resolution

A provider entry references an API by name and supplies connection details:

```yaml
providers:
  anthropic:
    url: https://api.anthropic.com/v1
    api: messages                 # references apis/messages.yaml
    api_key: ${ANTHROPIC_API_KEY}
    default_model: claude-3-5-sonnet-20241022
    hosted_models: /models
    hosted_model_fields: [id, type]
```

The **effective config** = the referenced API definition, with any inline `auth` / `request` / `response` / `tool_presets` on the provider **deep-merged over** the API blocks. Deep-merge semantics here (**provider-override merge**):

- nested dicts merge recursively,
- **lists are replaced** (not concatenated),
- scalars overwrite.

If a provider defines `request.body_schema` inline and has no `api:` reference, it is used as-is.

---

## 8. Versioning & extensibility

- This document is **v0** and freezes current behavior. Changes that alter composed bodies or extracted results are **breaking** and require a version bump plus new/updated conformance fixtures.
- Forward-compatible extension points (added in later versions, see [target-architecture.md](../target-architecture.md)): typed multimodal content parts in request/response, an `operation` lifecycle for async APIs, and a capability registry. v0 engines must preserve the behavior pinned here while those are layered on.
- Provider-specific fields that the schema does not model can still be injected via `set_paths` (escape hatch); unknown response fields are simply not extracted.

---

## 9. `content_mapping` (v0.1 — multimodal message content)

Each API def declares how canonical message content parts (see
[message-ir-v0.md](message-ir-v0.md)) lower into that provider's wire shape:

```yaml
content_mapping:
  text_only_as_string: false        # true -> a single text part renders as a bare string
  parts:
    text:
      template: { type: text, text: "{{text}}" }        # {{text}} from a TextPart
    image:
      template:                                          # {{url}} from an ImagePart
        type: image
        source: { type: url, url: "{{url}}" }
```

- The engine renders each part through its type's `template` (standard `{{var}}`
  substitution; a TextPart binds `{{text}}`, an ImagePart binds `{{url}}` and
  `{{media_type}}`), producing the provider's content.
- The message's `role` and the lowered `content` are supplied to the `messages`
  (or `input`) property's `x-render` template, which now binds `{{content}}`:

  ```yaml
  x-render:
    template:
      - role: "{{role}}"
        content: "{{content}}"
  ```

- `text_only_as_string: true` (Chat Completions) collapses a lone text part to a
  bare string; with any non-text part present, content is always a list.

This keeps text composition byte-identical to v0 while making image input
additive. The three shipped defs map: Anthropic → `text`/`image(source.url)`;
Chat Completions → `text`/`image_url` (string content when text-only); Responses
→ `input_text`/`input_image`.

---

## 10. `artifacts` — typed output (v0.1 — image generation)

APIs that produce **binary output** (generated images, audio, …) instead of text
declare an `artifacts` block in place of (or alongside) `text_extract`:

```yaml
response:
  artifacts:
    items_path: data        # list of artifact items in the response
    type: image             # artifact type assigned to each
    fields:                 # output field -> path within each item
      url: url
      b64: b64_json
      revised_prompt: revised_prompt
```

`extract_artifacts` walks `items_path`; for each item it emits
`{ "type": <type>, **fields }`, reading each field via its path **relative to the
item** and **omitting** any whose source resolves to null/absent. Returns `[]`
when nothing matches.

**Validation:** a `response` block must define `text_extract` *or* `artifacts`
(or both). `artifacts`, when present, must be a dict.

The shipped `image-generations` API family (OpenAI Images API) uses this with
`capabilities: { input: [text], output: [image] }` — so a non-vision caller is
rejected before the request, and the result is a list of typed image artifacts.
Binary payloads cross layer boundaries as **references** (a URL, or inline
base64), never as decoded bytes in the engine.
