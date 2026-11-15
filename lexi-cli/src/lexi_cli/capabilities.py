"""Capability registry: what a provider/model can accept and produce.

A capability descriptor declares the input and output **modalities** (``text``,
``image``, …) and optional ``limits`` for an API family, with optional per-model
overrides. Portable code queries capabilities to route, validate, and fail with
a clear message ("model does not accept image input") instead of an opaque
provider 4xx.

Pure: values in, values out. The only Lexi dependency is the message IR types.
See ``docs/spec/capabilities-v0.md``.
"""

from __future__ import annotations

from collections.abc import Mapping
from typing import Any

from lexi_cli.message_ir import Message


def resolve_capabilities(
    api_def: Mapping[str, Any],
    model_capabilities: Mapping[str, Any] | None = None,
) -> dict:
    """Resolve the effective capability descriptor for an API family + model.

    Starts from the API def's ``capabilities`` block; a per-model override
    replaces ``input``/``output`` wholesale and merges ``limits`` key-by-key.
    """
    base = api_def.get("capabilities") or {}
    cap: dict[str, Any] = {
        "input": list(base.get("input") or []),
        "output": list(base.get("output") or []),
        "limits": dict(base.get("limits") or {}),
    }
    if model_capabilities:
        if "input" in model_capabilities:
            cap["input"] = list(model_capabilities["input"] or [])
        if "output" in model_capabilities:
            cap["output"] = list(model_capabilities["output"] or [])
        if "limits" in model_capabilities:
            cap["limits"] = {**cap["limits"], **(model_capabilities["limits"] or {})}
    return cap


def supports_input(cap: Mapping[str, Any], modality: str) -> bool:
    """True if the capability descriptor accepts ``modality`` as input."""
    return modality in (cap.get("input") or [])


def supports_output(cap: Mapping[str, Any], modality: str) -> bool:
    """True if the capability descriptor can produce ``modality`` as output."""
    return modality in (cap.get("output") or [])


def validate_message(cap: Mapping[str, Any], message: Message) -> list[str]:
    """Return errors for any content part whose modality the model can't accept.

    One error per unsupported modality (deduplicated, in first-seen order).
    """
    errors: list[str] = []
    seen: set[str] = set()
    for part in message.content:
        modality = part.type
        if modality in seen:
            continue
        if not supports_input(cap, modality):
            seen.add(modality)
            errors.append(f"model does not accept {modality} input")
    return errors
