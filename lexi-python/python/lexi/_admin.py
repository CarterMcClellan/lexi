"""Shared plumbing for the admin modules."""

from typing import Any, Dict

from . import _lexi


def call(op: str, **kwargs: Any) -> Any:
    """Run admin operation ``op`` (``"group.verb"``), dropping ``None`` arguments."""
    args: Dict[str, Any] = {k: v for k, v in kwargs.items() if v is not None}
    return _lexi._admin(op, args)
