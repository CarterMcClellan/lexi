"""Background jobs submitted with :func:`lexi.submit` (``lexi jobs``)."""

from __future__ import annotations

from typing import Any, Dict

from . import _lexi


def status(provider: str, id: str) -> Dict[str, Any]:
    """Check a job once (``lexi jobs status``).

    Returns the operation ``{"id", "status", "state", "done", "provider"}``
    plus ``"text"`` once it has succeeded. ``state`` is one of ``pending``,
    ``running``, ``succeeded``, ``failed``, ``unknown``.
    """
    return _lexi._job_status(provider, id)


def wait(provider: str, id: str, *, interval: float = 1.0, timeout: float = 600.0) -> Dict[str, Any]:
    """Poll a job every ``interval`` seconds until it finishes (``lexi jobs wait``).

    Returns the finished operation with ``"text"``. Raises
    :class:`lexi.LexiError` if the job fails or ``timeout`` seconds pass first.
    """
    return _lexi._job_wait(provider, id, float(interval), float(timeout))
