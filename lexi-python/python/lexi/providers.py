"""Providers in ``providers/providers-config.yaml`` and their API keys (``lexi providers``)."""

from __future__ import annotations

from typing import Any, Dict, Iterable, List, Optional

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


def upgrade(name: str, *, template: Optional[str] = None, apply: bool = False) -> Dict[str, Any]:
    """Convert a provider to its shipped template (``lexi providers upgrade``),
    keeping only the values you changed. Without ``apply``, reports what would
    change: ``{provider, template, changed, before, after, dropped, kept}``;
    with it, also backs up and rewrites the config (``backup``)."""
    return call("providers.upgrade", name=name, template=template, apply=apply)


def apis(name: str) -> List[Dict[str, Any]]:
    """The APIs a provider offers: ``[{api, for?, default_model?, url?,
    enabled, source}]`` (``source``: ``template``, ``user`` or ``config``)."""
    return call("providers.apis", name=name)


def add_api(
    name: str,
    api: str,
    *,
    for_tags: Iterable[str] = (),
    default_model: Optional[str] = None,
    url: Optional[str] = None,
) -> Dict[str, Any]:
    """Add (or update) an API on a provider: models tagged with all of
    ``for_tags`` use it (none = the catch-all)."""
    tags = [*for_tags]
    return call("providers.add_api", name=name, api=api, default_model=default_model, url=url,
                **({"for": tags} if tags else {}))


def remove_api(name: str, api: str) -> Dict[str, Any]:
    """Remove an API from a provider (a template's API is turned off)."""
    return call("providers.remove_api", name=name, api=api)
