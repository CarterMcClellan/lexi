"""A provider's configured models, default model and aliases (``lexi models``)."""

from __future__ import annotations

from typing import Any, Dict, Iterable, List, Optional

from ._admin import call


def list(provider: str) -> List[Dict[str, Any]]:
    """Configured models: ``[{id, name, description, default, aliases, metadata,
    kind, api, tags}]`` — ``api`` is the API that serves the model, ``tags`` its
    effective tags."""
    return call("models.list", provider=provider)


def hosted(provider: str) -> List[Dict[str, Any]]:
    """The models the provider's API reports (a network call)."""
    return call("models.hosted", provider=provider)


def default(provider: str) -> Dict[str, Any]:
    """``{provider, default_model}`` (``None`` when unset)."""
    return call("models.default", provider=provider)


def set_default(provider: str, model: Optional[str]) -> Dict[str, Any]:
    """Make ``model`` the default (``None`` clears it)."""
    return call("models.set_default", provider=provider, model=model)


def add(
    provider: str,
    model: str,
    *,
    name: Optional[str] = None,
    description: Optional[str] = None,
    max_tokens: Optional[int] = None,
    context_window: Optional[int] = None,
    default: bool = False,
    **metadata: Any,
) -> Dict[str, Any]:
    """Add a model, or update the fields given on an existing one:
    ``{provider, model, existed}``. Extra keyword arguments are stored as
    metadata too; ``default=True`` makes it the provider's default."""
    fields = dict(metadata)
    for k, v in (("name", name), ("description", description), ("max_tokens", max_tokens),
                 ("context_window", context_window)):
        if v is not None:
            fields[k] = v
    return call("models.add", provider=provider, model=model, fields=fields, default=default)


set = add  # ``lexi models set`` — the same upsert


def remove(provider: str, model: str) -> Dict[str, Any]:
    """Remove a model (the default falls to the first remaining model)."""
    return call("models.remove", provider=provider, model=model)


def aliases(provider: str) -> List[Dict[str, Any]]:
    """``[{alias, model}]``, sorted by alias."""
    return call("models.aliases", provider=provider)


def set_alias(provider: str, alias: str, model: str) -> Dict[str, Any]:
    """Point ``alias`` at ``model``."""
    return call("models.set_alias", provider=provider, alias=alias, model=model)


def remove_alias(provider: str, alias: str) -> Dict[str, Any]:
    """Remove an alias."""
    return call("models.remove_alias", provider=provider, alias=alias)


def tag(
    provider: str,
    model: str,
    add: Iterable[str] = (),
    remove: Iterable[str] = (),
    *,
    clear: bool = False,
) -> Dict[str, Any]:
    """Add or remove your tags on a model id or glob (``"meta/*"``):
    ``tag("nvidia", "some/model", add=["kind:image-generation"], remove=["in:image"])``.
    ``clear=True`` removes all of your tags on it. Tags must be in the
    vocabulary (:func:`lexi.tags`); ``label:<name>`` is free-form."""
    if clear:
        return call("models.tag", provider=provider, model=model, clear=True)
    ops = ["+" + t for t in add] + ["-" + t for t in remove]
    return call("models.tag", provider=provider, model=model, ops=ops)


def tags(provider: str, model: str) -> Dict[str, Any]:
    """A model's classification: ``{provider, model, kind, api, binding,
    tags: [{tag, source}], error?}`` — ``error`` when no API serves it."""
    return call("models.tags", provider=provider, model=model)


def resolve(provider: str, model: Optional[str] = None, *, output: str = "text") -> Dict[str, Any]:
    """Which model and API a request would use: ``{provider, model, api, kind,
    tags}``. With no ``model``, the default for ``output`` (``"text"``,
    ``"image"``, …). Raises :class:`lexi.LexiError` when nothing serves it."""
    return call("models.resolve", provider=provider, model=model, output=output)
