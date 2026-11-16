"""App settings (``config.json``) (``lexi config``)."""

from __future__ import annotations

from typing import Any, Dict

from ._admin import call


def list() -> Dict[str, Any]:
    """Every setting: the defaults overlaid by ``config.json``."""
    return call("config.list")


def get(key: str) -> Any:
    """One setting's value (raises :class:`lexi.LexiError` if unset)."""
    return call("config.get", key=key)["value"]


def set(key: str, value: Any) -> Dict[str, Any]:
    """Save a setting: ``{key, value}``."""
    return call("config.set", key=key, value=value)


def remove(key: str) -> Dict[str, Any]:
    """Delete a setting from ``config.json``."""
    return call("config.remove", key=key)
