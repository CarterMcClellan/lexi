"""API definitions — shipped ones are built in; custom ones live in ``apis/`` (``lexi api``)."""

from __future__ import annotations

import os
from typing import Any, Dict, List, Mapping, Optional, Union

from ._admin import call


def list() -> List[Dict[str, Any]]:
    """Every definition: ``[{name, description, path, text_path, builtin}]``."""
    return call("apis.list")


def show(name: str) -> Dict[str, Any]:
    """The definition itself."""
    return call("apis.show", name=name)


def validate(name: str) -> Dict[str, Any]:
    """``{name, valid, errors}``."""
    return call("apis.validate", name=name)


def preview(
    name: Optional[str] = None,
    *,
    yaml: Optional[str] = None,
    prompt: str = "Sample prompt",
    model: str = "sample-model",
    role: str = "user",
) -> Dict[str, Any]:
    """What a definition (by ``name``, or raw ``yaml`` text) would send:
    ``{name, description, valid, errors, flags, presets, body | body_error}``."""
    return call("apis.preview", name=name, yaml=yaml, prompt=prompt, model=model, role=role)


def create(
    name: str,
    *,
    template: Optional[str] = None,
    path: Optional[str] = None,
    text_path: Optional[str] = None,
) -> Dict[str, Any]:
    """Create a custom definition, copying ``template`` (another definition's
    name) or scaffolding a chat-style one; ``path`` / ``text_path`` override
    the request path and response text path."""
    return call("apis.create", name=name, template=template, path=path, text_path=text_path)


def edit(
    name: str,
    *,
    path: Optional[str] = None,
    text_path: Optional[str] = None,
    metadata: Optional[Mapping[str, str]] = None,
) -> Dict[str, Any]:
    """Edit a custom definition: request path, response text path, and
    ``response.metadata`` entries (``{key: path}``)."""
    return call("apis.edit", name=name, path=path, text_path=text_path,
                metadata=dict(metadata) if metadata is not None else None)


def save(name: str, yaml: Union[str, "os.PathLike[str]"]) -> Dict[str, Any]:
    """Save raw YAML as a custom definition — ``yaml`` is the text, or a
    ``pathlib.Path`` to read it from: ``{saved, valid, errors}``."""
    if isinstance(yaml, os.PathLike):
        with open(yaml, encoding="utf-8") as f:
            yaml = f.read()
    return call("apis.save", name=name, yaml=yaml)


def remove(name: str) -> Dict[str, Any]:
    """Delete a custom definition."""
    return call("apis.remove", name=name)
