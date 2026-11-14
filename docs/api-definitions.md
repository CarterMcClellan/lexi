# API Definitions

Lexi uses a config-driven API abstraction that separates **how to call an LLM API** from **which provider you're calling**. Each API style is defined in a standalone YAML file — no code changes are needed to support a new API format.

## Overview

Three API definitions are bundled by default:

| Name | Description | Used by |
|---|---|---|
| `chat-completions` | OpenAI-compatible Chat Completions (`/chat/completions`) | NVIDIA |
| `messages` | Anthropic Messages API (`/messages`) | Anthropic |
| `responses` | OpenAI Responses API (`/responses`) | OpenAI |

API definitions live in `~/.lexi-cli/apis/` and are seeded from the bundled defaults on first run.

Providers reference an API definition by name in `providers-config.yaml`:

```yaml
providers:
  openai:
    url: https://api.openai.com/v1
    api_key: ${OPENAI_API_KEY}
    api: responses

  anthropic:
    url: https://api.anthropic.com/v1
    api_key: ${ANTHROPIC_API_KEY}
    api: messages
```

## Anatomy of an API Definition

Here's the complete `chat-completions.yaml` as an example:

```yaml
name: chat-completions
description: "OpenAI-compatible Chat Completions API"

auth:
  header: Authorization
  value_prefix: "Bearer "

request:
  method: POST
  path: /chat/completions
  body:
    model: "{{model}}"
    messages:
      - role: "{{role}}"
        content: "{{prompt}}"
  parameters:
    temperature:
      body_field: temperature
      type: float
      range: [0.0, 2.0]
      default: 0.7
      description: "Sampling temperature"
    max_tokens:
      body_field: max_tokens
      type: int
      range: [1, 131072]
      default: 4096
      description: "Maximum tokens to generate"
  tools:
    key: tools

response:
  text_path: "choices[0].message.content"
  metadata:
    model: "model"
    input_tokens: "usage.prompt_tokens"
    output_tokens: "usage.completion_tokens"
    total_tokens: "usage.total_tokens"
    stop_reason: "choices[0].finish_reason"
```

### `auth` Block

Defines how to authenticate API requests.

| Field | Description | Example |
|---|---|---|
| `header` | HTTP header name for the API key | `Authorization`, `x-api-key` |
| `value_prefix` | String prepended to the API key value | `"Bearer "` (note trailing space) |
| `extra_headers` | Additional headers to include | `anthropic-version: "2023-06-01"` |

### `request` Block

Defines how to build the HTTP request.

**`method`** — HTTP method (default: `POST`).

**`path`** — URL path appended to the provider's base URL (e.g., `/chat/completions`).

**`body`** — Template for the JSON request body. Uses `{{variable}}` placeholders:

| Variable | Description |
|---|---|
| `{{model}}` | The model ID to use |
| `{{role}}` | The message role (e.g., `system`, `user`) |
| `{{prompt}}` | The prompt text |

The body template can be any nested structure — Lexi recursively substitutes variables in strings, dicts, and lists.

**`parameters`** — Typed parameters that map to fields in the request body:

```yaml
parameters:
  temperature:
    body_field: temperature    # Key name in the JSON body
    type: float                # float, int, str, bool
    range: [0.0, 2.0]         # Optional min/max for validation
    default: 0.7              # Default value
    description: "..."        # Human-readable description
```

When you pass `--temperature 0.5` to `respond`, Lexi looks up the `temperature` parameter, finds its `body_field`, and sets that key in the request body. The `range` is used for validation warnings.

**`tools`** — Optional tools/function-calling config:

```yaml
tools:
  key: tools                   # Key name for the tools array in the body
```

### `response` Block

Defines how to extract data from the API response.

**`text_path`** — Dot/bracket path to the response text. Lexi walks this path through the JSON response to extract the generated text:

| API | text_path |
|---|---|
| Chat Completions | `choices[0].message.content` |
| Messages | `content[0].text` |
| Responses | `output[0].content[0].text` |

**`metadata`** — Paths to extract additional response metadata:

```yaml
metadata:
  model: "model"                        # Model that was used
  input_tokens: "usage.prompt_tokens"   # Input token count
  output_tokens: "usage.completion_tokens"
  total_tokens: "usage.total_tokens"
  stop_reason: "choices[0].finish_reason"
```

## Provider Overrides

A provider can include inline `auth`, `request`, or `response` blocks that are **deep-merged** on top of the referenced API definition. This lets you customize specific fields without duplicating the entire definition.

For example, to use chat-completions but with a different request path:

```yaml
providers:
  my-provider:
    url: https://api.example.com
    api_key: ${MY_API_KEY}
    api: chat-completions
    request:
      path: /v2/completions    # Overrides /chat/completions from the API def
```

The merge is recursive — nested dicts are merged, other values are replaced.

## Managing API Definitions

### List Definitions

```bash
api list                    # Formatted list
api list --raw              # Raw JSON output
```

### View a Definition

```bash
api show chat-completions          # Formatted view
api show messages --raw            # Raw YAML
```

### Create a New Definition

Create from scratch (starts with a chat-completions-style template):

```bash
api create my-api --path /v1/generate --text-path "result.text"
```

Clone an existing definition:

```bash
api create my-api --from messages
```

### Edit a Definition

```bash
# Change the request path
api edit my-api --path /v2/generate

# Change the response text extraction path
api edit my-api --text-path "output.text"

# Add or modify a parameter (NAME:FIELD:TYPE:DEFAULT)
api edit my-api --set-param temperature:temperature:float:0.7

# Add or modify a metadata extraction path (KEY:PATH)
api edit my-api --set-meta total_tokens:usage.total_tokens
```

### Validate a Definition

Check that a definition has all required fields and valid structure:

```bash
api validate my-api
```

Validation checks:
- `name` field is present and non-empty
- `request` block exists with `path` and `body`
- Parameters have valid types (`float`, `int`, `str`, `bool`) and ranges
- `response` block exists with `text_path`
- Metadata paths are well-formed

### Delete a Definition

```bash
api rm my-api
```

## Tutorial: Adding a New Provider

Let's say you want to add a provider that uses the standard OpenAI Chat Completions format (many providers do).

### Step 1: Add the Provider

```bash
providers add my-llm --url https://api.my-llm.com/v1 --api_key '${MY_LLM_API_KEY}' --default_model my-model-v1
```

Then set the API binding by editing `~/.lexi-cli/providers/providers-config.yaml`:

```yaml
providers:
  my-llm:
    url: https://api.my-llm.com/v1
    api_key: ${MY_LLM_API_KEY}
    api: chat-completions        # Uses the existing chat-completions definition
```

### Step 2: Set Your API Key

```bash
export MY_LLM_API_KEY="your-key-here"
# Or:
providers api-key set my-llm --value your-key-here
```

### Step 3: Test It

```bash
providers status my-llm
prompts set test --prompt "Hello, world!"
respond my-llm test
```

### Adding a Provider with a Custom API

If the provider uses a non-standard API format, create a new API definition first:

```bash
# Start from the closest existing definition
api create my-custom-api --from chat-completions

# Adjust the request path and response extraction
api edit my-custom-api --path /api/generate --text-path "response.content"

# Validate it
api validate my-custom-api
```

Then reference it in your provider config:

```yaml
providers:
  my-provider:
    url: https://api.example.com
    api_key: ${MY_API_KEY}
    api: my-custom-api
```

## Config Resolution Pipeline

When Lexi sends a request, it resolves the effective configuration in this order:

1. **Load API definition** — If the provider has `api: <name>`, load that YAML file
2. **Deep-merge provider overrides** — Any inline `auth`/`request`/`response` blocks in the provider config are merged on top
3. **Legacy fallback** — If no `api` key and no `request` block, fall back to the legacy `respond_kind`/`respond_path` migration

This means the API definition provides the base configuration, the provider can customize it, and old-format configs continue to work.
