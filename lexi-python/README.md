# lexi (Python wheel)

Use multiple LLM providers side by side from Python. A thin
[PyO3](https://pyo3.rs) binding over the same Rust core as the `lexi` CLI and
desktop app, not a separate reimplementation. It reads the same config,
API keys, saved prompts and model aliases in `~/.lexi-cli` (or `$LEXI_HOME`),
and fills in any missing defaults the first time it is used, like the CLI.

## Send prompts

```python
import lexi

r = lexi.respond("openai", "Say hello")
r["text"], r["model"], r["metadata"]          # metadata: token counts, stop reason, …

lexi.respond("anthropic", prompt="review")     # a saved prompt (default: the active one)
lexi.respond("openai", "Summarize", model="mini", images=["chart.png"],
             set={"temperature": 0.2}, tools=["web_search"], cite=True)

for ev in lexi.stream("anthropic", "Count to five"):
    if ev["event"] == "delta":
        print(ev["text"], end="", flush=True)
    elif ev["event"] == "provider_done":
        print("\n", ev["metadata"])

job = lexi.submit("openai", "Write a long report")      # background job, returns at once
lexi.jobs.status("openai", job["id"])                    # {"state": "running", …}
done = lexi.jobs.wait("openai", job["id"], timeout=900)  # … {"state": "succeeded", "text": …}

# images: the model's tags (or output="image") pick the provider's image API
r = lexi.respond("openai", "a watercolor fox", output="image", out_dir="art")
r["artifacts"]                                  # [{path: "art/a-watercolor-fox-1.png", …}]
lexi.generate("openai", "a watercolor fox", set={"size": "1536x864"})   # same thing
up = lexi.upload("openai", "diagram.png")
lexi.respond("openai", "Describe this", images=[f"file:{up['file_id']}"])

lexi.prepare("openai", "hi", mode="stream")["body"]   # the request body, not sent
```

| Function | CLI equivalent |
|---|---|
| `respond(provider, text=None, *, prompt, role, model, images, params, tools, set, unset, validate, background, cite, raw, output, out_dir)` → text **and** `artifacts` (saved files) | `lexi respond` |
| `stream(...)` (same options) → iterator of events | `lexi respond --stream --json` |
| `submit(...)` → operation | `lexi respond --background --detach` |
| `jobs.status(provider, id)` / `jobs.wait(provider, id, interval=1, timeout=600)` | `lexi jobs status` / `lexi jobs wait` |
| `generate(provider, prompt, *, model, out_dir=".")` — `respond(..., output="image")` | `lexi generate` |
| `upload(provider, path)` | `lexi upload` |
| `prepare(...)` → `{"provider", "model", "body"}` | — (compose only) |

## Manage the config

The same operations as the CLI's admin commands, over the same files:

```python
lexi.providers.list()                         # [{name, url, api, default_model, key_source, …}]
lexi.providers.add("local", "http://localhost:8000/v1", "llama-3", api_key="…")
lexi.providers.status("openai")               # {"status": "ready", "source": "env", …}
lexi.providers.set_key("openai", "sk-…")      # in a 0600 key file (where api_key says), never the config itself

lexi.models.list("openai"); lexi.models.hosted("openai")
lexi.models.add("openai", "gpt-4.1", name="GPT-4.1", default=True)
lexi.models.set_alias("openai", "fast", "gpt-4.1-mini")

lexi.prompts.save("review", "Review this code", temperature=0.2)
lexi.prompts.activate("review")               # the default for respond()
lexi.config.set("edit_mode", "emacs")

lexi.apis.list(); lexi.apis.validate("mine")
lexi.apis.create("mine", template="chat-completions", path="/v2/chat")
lexi.apis.preview("mine", prompt="hi", model="m")["body"]
```

| Module | Functions | CLI |
|---|---|---|
| `lexi.providers` | `list`, `status`, `add`, `rename`, `remove`, `set_key`, `show_key`, `remove_key` | `lexi providers …` |
| `lexi.models` | `list`, `hosted`, `default`, `set_default`, `add` / `set`, `remove`, `aliases`, `set_alias`, `remove_alias` | `lexi models …` |
| `lexi.prompts` | `list`, `get`, `save`, `remove`, `activate` | `lexi prompts …` |
| `lexi.config` | `list`, `get`, `set`, `remove` | `lexi config …` |
| `lexi.apis` | `list`, `show`, `validate`, `preview`, `create`, `edit`, `save`, `remove` | `lexi api …` |

## Results and errors

Results are plain dicts in the same shapes as the CLI's `--json` output
([`docs/spec/cli-machine-v1.md`](../docs/spec/cli-machine-v1.md)), built by the
same code. Failures raise `lexi.LexiError`; bad arguments raise `ValueError`.
Calls release the GIL while they wait, so threads run concurrently and Ctrl-C
interrupts a call in progress.

## The engine

The pure request/response engine works on API definitions directly: no config,
no network.

```python
errors  = lexi.validate_definition(api_def)        # [] when valid
flags   = lexi.cli_flags(api_def)                  # CLI flag specs from x-cli props
preview = lexi.preview_definition(api_def, "hello", "gpt-4o", "user")
body    = lexi.compose_request_body(api_def, prompt_payload, overrides)
text    = lexi.extract_text(response_json, api_def["response"]["text_extract"])
```

Its behavior is pinned by the cross-language conformance suite
(`conformance/`), run from both Rust and this wheel.

The wheel is `abi3` (one build works on CPython 3.9+).

## Install

Prebuilt wheels (macOS arm64/x86_64, Linux x86_64/aarch64, Windows x64) and an
sdist are attached to each [GitHub Release](https://github.com/badscooter23/lexi/releases).
Download the wheel for your platform and:

```sh
pip install ./lexi-0.3.0-cp39-abi3-<platform>.whl
```

Or build from source (needs a Rust toolchain):

```sh
pip install ./lexi-python          # from a checkout
# or
maturin build --release --manifest-path lexi-python/Cargo.toml
```

> Not published to PyPI — the `lexi` name there belongs to an unrelated project.
> Install from the GitHub Release artifacts (above) instead.

## Tests

```sh
pip install ./lexi-python pytest pyyaml && pytest lexi-python/tests
```

`test_conformance_wheel.py` runs the shared conformance fixtures;
`test_runtime.py` drives the runtime against an in-process mock provider.
