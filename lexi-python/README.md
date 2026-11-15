# lexi (Python wheel)

The **Lexi engine** — a provider-agnostic, schema-driven LLM API layer — exposed
to Python. It's a thin [PyO3](https://pyo3.rs) binding over the Rust `lexi-core`
crate (not a separate reimplementation), so Python and Rust share one engine
whose behavior is pinned by the cross-language conformance suite.

```python
import lexi

# Validate / introspect a schema-driven API definition
errors = lexi.validate_definition(api_def)        # [] when valid
flags  = lexi.cli_flags(api_def)                  # CLI flag specs from x-cli props
preview = lexi.preview_definition(api_def, "hello", "gpt-4o", "user")

# Compose a request body / extract a response (the core request/response engine)
body = lexi.compose_request_body(api_def, prompt_payload, overrides, [], [], [], False)
text = lexi.extract_text(response_json, api_def["response"]["text_extract"])
```

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
