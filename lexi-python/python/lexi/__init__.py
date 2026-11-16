"""Lexi — use multiple LLM providers side by side from Python.

Two layers, one Rust core:

* **Runtime** — send prompts to the providers configured in ``~/.lexi-cli``
  (or ``$LEXI_HOME``), the same config, API keys, prompts and model aliases the
  ``lexi`` CLI and desktop app use::

      import lexi
      r = lexi.respond("openai", "Say hello")          # {"text", "model", "metadata", ...}
      for ev in lexi.stream("anthropic", "Count to 5"):
          if ev["event"] == "delta":
              print(ev["text"], end="")
      job = lexi.submit("openai", "Write a long essay")
      done = lexi.jobs.wait("openai", job["id"])

  Results are the CLI's ``--json`` shapes (``docs/spec/cli-machine-v0.md``);
  failures raise :class:`LexiError`.

* **Engine** — the pure request/response engine over API definitions
  (``compose_request_body``, ``extract_text``, ``validate_definition``, …), no
  network or config involved.
"""

from __future__ import annotations

import os
from typing import Any, Dict, Iterable, Iterator, Mapping, Optional, Sequence, Tuple, Union

from . import _lexi
from ._lexi import *  # noqa: F401,F403 — the engine functions
from ._lexi import ChatStream, LexiError

__all__ = [
    "LexiError",
    "ChatStream",
    "respond",
    "stream",
    "submit",
    "prepare",
    "generate",
    "upload",
    "jobs",
] + [n for n in dir(_lexi) if not n.startswith("_") and n not in ("LexiError", "ChatStream")]

SetPaths = Union[Mapping[str, Any], Sequence[Tuple[str, Any]]]


def _opts(
    text: Optional[str],
    prompt: Optional[str],
    role: Optional[str],
    model: Optional[str],
    images: Optional[Iterable[Union[str, "os.PathLike[str]"]]],
    params: Optional[Mapping[str, Any]],
    tools: Optional[Iterable[str]],
    set: Optional[SetPaths],
    unset: Optional[Iterable[str]],
    validate: bool,
    cite: bool,
    raw: bool,
) -> Dict[str, Any]:
    opts: Dict[str, Any] = {
        "text": text,
        "prompt": prompt,
        "role": role,
        "model": model,
        "images": [os.fspath(i) for i in images] if images is not None else None,
        "params": dict(params) if params is not None else None,
        "tools": list(tools) if tools is not None else None,
        "set": (dict(set) if isinstance(set, Mapping) else [list(p) for p in set])
        if set is not None
        else None,
        "unset": list(unset) if unset is not None else None,
        "validate": validate,
        "cite": cite,
        "raw": raw,
    }
    return {k: v for k, v in opts.items() if v is not None}


def respond(
    provider: str,
    text: Optional[str] = None,
    *,
    prompt: Optional[str] = None,
    role: Optional[str] = None,
    model: Optional[str] = None,
    images: Optional[Iterable[Union[str, "os.PathLike[str]"]]] = None,
    params: Optional[Mapping[str, Any]] = None,
    tools: Optional[Iterable[str]] = None,
    set: Optional[SetPaths] = None,
    unset: Optional[Iterable[str]] = None,
    validate: bool = True,
    background: bool = False,
    cite: bool = False,
    raw: bool = False,
) -> Dict[str, Any]:
    """Send a prompt and wait for the reply (``lexi respond``).

    The prompt is ``text``, else the saved prompt named ``prompt``, else the
    active saved prompt. ``role`` overrides its role.

    * ``model`` — a configured model id or alias (default: the provider's default).
    * ``images`` — image URLs, local paths, or ``"file:<id>"`` for an uploaded file.
    * ``params`` — request-body overrides (like ``--params``).
    * ``tools`` — tool preset names from the API definition (like ``--tool``).
    * ``set`` / ``unset`` — set or remove body paths (``{"temperature": 0.2}``,
      ``["system"]``), applied last.
    * ``validate=False`` — skip validating the body against the API schema.
    * ``background=True`` — run as a background job and poll until done (see
      :func:`submit` to return immediately instead).

    Returns ``{"provider", "model", "text", "metadata", "activity"?,
    "sources"?, "raw"?}`` — ``sources`` (rendered citations) with ``cite``,
    the provider's raw response with ``raw``.
    """
    o = _opts(text, prompt, role, model, images, params, tools, set, unset, validate, cite, raw)
    return _lexi._respond(provider, o, background)


def stream(
    provider: str,
    text: Optional[str] = None,
    *,
    prompt: Optional[str] = None,
    role: Optional[str] = None,
    model: Optional[str] = None,
    images: Optional[Iterable[Union[str, "os.PathLike[str]"]]] = None,
    params: Optional[Mapping[str, Any]] = None,
    tools: Optional[Iterable[str]] = None,
    set: Optional[SetPaths] = None,
    unset: Optional[Iterable[str]] = None,
    validate: bool = True,
    cite: bool = False,
    raw: bool = False,
) -> Iterator[Dict[str, Any]]:
    """Send a prompt and stream the reply (``lexi respond --stream --json``).

    Takes the same arguments as :func:`respond` and yields event dicts as they
    arrive::

        {"event": "provider_start", "provider": ...}
        {"event": "activity", "provider": ..., "message": "searching the web"}
        {"event": "delta", "provider": ..., "text": "Hel"}
        {"event": "provider_done", "provider", "model", "text", "metadata", ...}

    ``provider_done`` carries the full result (as :func:`respond` returns it).
    A failure raises :class:`LexiError`. The request starts when ``stream()``
    is called; ``close()`` the iterator (or drop it) to stop early.
    """
    o = _opts(text, prompt, role, model, images, params, tools, set, unset, validate, cite, raw)
    return _lexi._stream(provider, o)


def submit(
    provider: str,
    text: Optional[str] = None,
    *,
    prompt: Optional[str] = None,
    role: Optional[str] = None,
    model: Optional[str] = None,
    images: Optional[Iterable[Union[str, "os.PathLike[str]"]]] = None,
    params: Optional[Mapping[str, Any]] = None,
    tools: Optional[Iterable[str]] = None,
    set: Optional[SetPaths] = None,
    unset: Optional[Iterable[str]] = None,
    validate: bool = True,
) -> Dict[str, Any]:
    """Submit a prompt as a background job and return at once
    (``lexi respond --background --detach``).

    Returns the operation ``{"id", "status", "state", "done", "provider"}``;
    follow it with :func:`lexi.jobs.status` or :func:`lexi.jobs.wait`. Needs an
    API with a background lifecycle (e.g. OpenAI's Responses API).
    """
    o = _opts(text, prompt, role, model, images, params, tools, set, unset, validate, False, False)
    return _lexi._submit(provider, o)


def prepare(
    provider: str,
    text: Optional[str] = None,
    *,
    prompt: Optional[str] = None,
    role: Optional[str] = None,
    model: Optional[str] = None,
    images: Optional[Iterable[Union[str, "os.PathLike[str]"]]] = None,
    params: Optional[Mapping[str, Any]] = None,
    tools: Optional[Iterable[str]] = None,
    set: Optional[SetPaths] = None,
    unset: Optional[Iterable[str]] = None,
    validate: bool = True,
    mode: str = "buffered",
) -> Dict[str, Any]:
    """Resolve the model and compose the request body without sending it.

    Same arguments as :func:`respond`; ``mode`` is ``"buffered"``,
    ``"stream"`` or ``"background"``. Returns ``{"provider", "model", "body"}``.
    Needs the provider's API key to be configured, but makes no network call.
    """
    o = _opts(text, prompt, role, model, images, params, tools, set, unset, validate, False, False)
    return _lexi._prepare(provider, o, mode)


def generate(
    provider: str,
    prompt: str,
    *,
    model: Optional[str] = None,
    out_dir: Union[str, "os.PathLike[str]"] = ".",
) -> Dict[str, Any]:
    """Generate images and save them under ``out_dir`` (``lexi generate``).

    Returns ``{"provider", "artifacts": [{"path", "revised_prompt"}]}``.
    """
    return _lexi._generate(provider, prompt, model, os.fspath(out_dir))


def upload(provider: str, path: Union[str, "os.PathLike[str]"]) -> Dict[str, Any]:
    """Upload a file to the provider (``lexi upload``).

    Returns ``{"provider", "file_id"}``; pass ``f"file:{file_id}"`` in
    ``images=`` to use an uploaded image.
    """
    return _lexi._upload(provider, os.fspath(path))


from . import jobs  # noqa: E402 — after the functions it doesn't depend on
