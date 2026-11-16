"""Providers in ``providers/providers-config.yaml`` and their API keys (``lexi providers``)."""

from __future__ import annotations

from typing import Any, Dict, List, Optional

from ._admin import call


def list() -> List[Dict[str, Any]]:
    """Every provider: ``{name, url, api, default_model, hosted_models,
    hosted_model_fields, key_source}``; ``key_source`` is ``"file"``, ``"env"``,
    ``"config"`` or ``None`` (the key itself is never included)."""
    return call("providers.list")


def status(name: str) -> Dict[str, Any]:
    """Check the provider's API key against its models endpoint:
    ``{provider, status, source}``, ``status`` one of ``"ready"``,
    ``"no api-key"``, ``"invalid api-key"``."""
    return call("providers.status", name=name)


def add(
    name: str,
    url: str,
    default_model: str,
    *,
    api: str = "chat-completions",
    api_key: Optional[str] = None,
    hosted_models: str = "/models",
) -> Dict[str, Any]:
    """Add a provider speaking API definition ``api`` at ``url``. ``api_key``
    goes to the provider's owner-only key file, never the config."""
    return call(
        "providers.add",
        name=name,
        url=url,
        default_model=default_model,
        api=api,
        api_key=api_key,
        hosted_models=hosted_models,
    )


def rename(old: str, new: str) -> Dict[str, Any]:
    """Rename a provider (its models, aliases and key file move with it)."""
    return call("providers.rename", old=old, new=new)


def remove(name: str) -> Dict[str, Any]:
    """Remove a provider and its per-provider files."""
    return call("providers.remove", name=name)


def set_key(name: str, key: str) -> Dict[str, Any]:
    """Store an API key in the provider's key file (mode 0600)."""
    return call("providers.set_key", name=name, key=key)


def show_key(name: str, *, reveal: bool = False) -> Dict[str, Any]:
    """``{provider, source, key}`` — the key masked unless ``reveal``."""
    return call("providers.show_key", name=name, reveal=reveal)


def remove_key(name: str) -> Dict[str, Any]:
    """Delete the provider's key file: ``{"removed": bool}``."""
    return call("providers.remove_key", name=name)
