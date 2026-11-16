"""Saved prompts (``prompts/*.json``; the active prompt is named ``$$``) (``lexi prompts``)."""

from __future__ import annotations

from typing import Any, Dict, List, Optional

from ._admin import call

ACTIVE = "$$"
"""The name the active prompt is saved under."""


def list() -> List[Dict[str, Any]]:
    """Every saved prompt: ``[{name, active, prompt, max_tokens, temperature, role, …}]``,
    the active prompt first."""
    return call("prompts.list")


def get(name: str) -> Dict[str, Any]:
    """One saved prompt (raises :class:`lexi.LexiError` if there is none)."""
    return call("prompts.get", name=name)


def save(
    name: str,
    text: Optional[str] = None,
    *,
    max_tokens: Optional[int] = None,
    temperature: Optional[float] = None,
    role: Optional[str] = None,
    **fields: Any,
) -> Dict[str, Any]:
    """Create or update a saved prompt (``name=lexi.prompts.ACTIVE`` for the
    active one). Fields not given keep their saved values (or the defaults:
    256 tokens, temperature 0.7, role ``user``). Returns the saved prompt."""
    for k, v in (("prompt", text), ("max_tokens", max_tokens), ("temperature", temperature),
                 ("role", role)):
        if v is not None:
            fields[k] = v
    return call("prompts.save", name=name, fields=fields)


def remove(name: str) -> Dict[str, Any]:
    """Delete a saved prompt."""
    return call("prompts.remove", name=name)


def activate(name: str) -> Dict[str, Any]:
    """Make a saved prompt the active one (copies it to ``$$``)."""
    return call("prompts.activate", name=name)
