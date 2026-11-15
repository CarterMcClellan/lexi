"""Async operation lifecycle: submit -> poll -> fetch result.

Some provider APIs are asynchronous: a submit returns an operation handle and an
initial status; the caller polls until a terminal state, then reads the result
from the final response (via the usual ``text_extract`` / ``artifacts``). This
module **interprets** an API def's ``operation`` block — reading the handle and
status and classifying it into canonical states, and building the poll path. The
polling loop and HTTP live in the host (CLI/GUI); this module is pure.

Canonical states: ``pending``, ``running``, ``succeeded``, ``failed``,
``unknown``. ``succeeded``/``failed`` are terminal.

See ``docs/spec/operations-v0.md``.
"""

from __future__ import annotations

from collections.abc import Mapping
from typing import Any

from lexi_cli.api_compose import get_path

CANONICAL_STATES = ("pending", "running", "succeeded", "failed")
TERMINAL_STATES = ("succeeded", "failed")


def classify_status(status_value: Any, status_map: Mapping[str, Any]) -> str:
    """Map a raw provider status onto a canonical state (``unknown`` if unmatched)."""
    if status_value is None:
        return "unknown"
    for state in CANONICAL_STATES:
        if status_value in (status_map.get(state) or []):
            return state
    return "unknown"


def read_operation(response: Any, operation_cfg: Mapping[str, Any] | None) -> dict:
    """Read an operation handle/status from a submit or poll response.

    Returns ``{id, status, state, done}``. ``id`` is the raw operation id (or
    ``None``), ``status`` the raw provider status, ``state`` its canonical
    classification, and ``done`` whether the state is terminal.
    """
    if not operation_cfg:
        return {"id": None, "status": None, "state": "unknown", "done": False}
    op_id = get_path(response, operation_cfg.get("id_path", "id"))
    status = get_path(response, operation_cfg.get("status_path", "status"))
    state = classify_status(status, operation_cfg.get("status_map") or {})
    return {"id": op_id, "status": status, "state": state, "done": state in TERMINAL_STATES}


def poll_path(operation_cfg: Mapping[str, Any], operation_id: Any) -> str:
    """Build the poll path for ``operation_id`` by substituting ``{id}``."""
    template = (operation_cfg.get("poll") or {}).get("path_template", "")
    return template.replace("{id}", "" if operation_id is None else str(operation_id))


def is_terminal(state: str) -> bool:
    """True if ``state`` is a terminal canonical state (``succeeded``/``failed``)."""
    return state in TERMINAL_STATES
