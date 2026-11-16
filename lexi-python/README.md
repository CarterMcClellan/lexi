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

# generate: a provider whose `api` is image-generations; files are saved to art/
lexi.generate("images", "a watercolor fox", out_dir="art")
up = lexi.upload("openai", "diagram.png")
lexi.respond("openai", "Describe this", images=[f"file:{up['file_id']}"])

lexi.prepare("openai", "hi", mode="stream")["body"]   # the request body, not sent
```

| Function | CLI equivalent |
|---|---|
| `respond(provider, text=None, *, prompt, role, model, images, params, tools, set, unset, validate, background, cite, raw)` | `lexi respond` |
| `stream(...)` (same options) → iterator of events | `lexi respond --stream --json` |
| `submit(...)` → operation | `lexi respond --background --detach` |
| `jobs.status(provider, id)` / `jobs.wait(provider, id, interval=1, timeout=600)` | `lexi jobs status` / `lexi jobs wait` |
| `generate(provider, prompt, *, model, out_dir=".")` | `lexi generate` |
| `upload(provider, path)` | `lexi upload` |
| `prepare(...)` → `{"provider", "model", "body"}` | — (compose only) |

Results are plain dicts in the same shapes as the CLI's `--json` output
([`docs/spec/cli-machine-v0.md`](../docs/spec/cli-machine-v0.md)), built by the
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
pip install ./lexi-0.1.0-cp39-abi3-<platform>.whl
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
