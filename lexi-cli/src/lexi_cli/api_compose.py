"""CLI-side helpers retained after the engine moved to Rust (`lexi` wheel).

The schema-driven engine (request composition, response extraction, template
rendering, validation, message IR) now lives in `lexi-core` and is reached from
Python via the `lexi` wheel. This module keeps only the helpers the CLI needs
that are *not* part of that engine surface:

* ``parse_path`` / ``set_path`` / ``unset_path`` — dotted/bracketed path edits
  used by the CLI's ``--set`` / ``--unset`` prompt-payload manipulation.
* ``cli_flags_for`` / ``tool_preset_names`` — introspect an API definition to
  drive CLI flags and preset listings.
* ``_VALID_INCLUDES`` — the set of valid ``x-include`` modes, used by
  ``validate_api_definition``.

The engine's behavior is pinned by the shared conformance suite, run against the
Rust core (``lexi-core/tests``) and its wheel (``lexi-python/tests``).
"""

from __future__ import annotations

import re
from collections.abc import Mapping
from typing import Any

# ---------------------------------------------------------------------------
# Path helpers (dotted/bracketed access, e.g. "reasoning.effort", "messages[0]")
# ---------------------------------------------------------------------------

_PATH_TOKEN = re.compile(r"([^.\[\]]+)|\[(\d+)\]")


def parse_path(path: str) -> list:
    """Parse a dotted/bracketed path into segments. ``"a.b[0].c"`` → ``['a','b',0,'c']``."""
    segments: list = []
    for match in _PATH_TOKEN.finditer(path):
        key, idx = match.groups()
        if key is not None:
            segments.append(key)
        elif idx is not None:
            segments.append(int(idx))
    return segments


def set_path(obj: Any, path: str, value: Any) -> None:
    """Set ``value`` at ``path``, creating intermediate dicts/lists as needed."""
    segs = parse_path(path)
    if not segs:
        raise ValueError("empty path")
    cur = obj
    for i, seg in enumerate(segs[:-1]):
        nxt = segs[i + 1]
        if isinstance(seg, int):
            if not isinstance(cur, list):
                raise TypeError(f"set_path: expected list at segment {i} of {path!r}")
            while len(cur) <= seg:
                cur.append(None)
            if cur[seg] is None or (
                isinstance(nxt, int) and not isinstance(cur[seg], list)
            ) or (isinstance(nxt, str) and not isinstance(cur[seg], dict)):
                cur[seg] = [] if isinstance(nxt, int) else {}
            cur = cur[seg]
        else:
            if not isinstance(cur, dict):
                raise TypeError(f"set_path: expected dict at segment {i} of {path!r}")
            if seg not in cur or cur[seg] is None or (
                isinstance(nxt, int) and not isinstance(cur[seg], list)
            ) or (isinstance(nxt, str) and not isinstance(cur[seg], dict)):
                cur[seg] = [] if isinstance(nxt, int) else {}
            cur = cur[seg]
    last = segs[-1]
    if isinstance(last, int):
        if not isinstance(cur, list):
            raise TypeError(f"set_path: expected list at end of {path!r}")
        while len(cur) <= last:
            cur.append(None)
        cur[last] = value
    else:
        if not isinstance(cur, dict):
            raise TypeError(f"set_path: expected dict at end of {path!r}")
        cur[last] = value


def unset_path(obj: Any, path: str) -> None:
    """Remove the value at ``path``. No-op if the path does not exist."""
    segs = parse_path(path)
    if not segs:
        return
    cur = obj
    for seg in segs[:-1]:
        if isinstance(seg, int):
            if not isinstance(cur, list) or seg >= len(cur):
                return
            cur = cur[seg]
        else:
            if not isinstance(cur, dict) or seg not in cur:
                return
            cur = cur[seg]
    last = segs[-1]
    if isinstance(last, int):
        if isinstance(cur, list) and 0 <= last < len(cur):
            cur.pop(last)
    elif isinstance(cur, dict) and last in cur:
        del cur[last]


# ---------------------------------------------------------------------------
# Valid x-include modes (used by validate_api_definition)
# ---------------------------------------------------------------------------

_VALID_INCLUDES = {
    "always",
    "if_set",
    "if_truthy",
    "if_non_empty",
    "if_any_child_set",
}


# ---------------------------------------------------------------------------
# CLI flag / preset introspection
# ---------------------------------------------------------------------------


def cli_flags_for(api_def: Mapping[str, Any]) -> list[dict]:
    """Walk the body_schema and return a flat list of CLI flag specs.

    Each spec contains: flag, alias, path, type, enum, default, description,
    repeat, invert, enum_map.
    """
    flags: list[dict] = []
    body_schema = (api_def.get("request") or {}).get("body_schema") or {}
    _collect_flags(body_schema.get("properties") or {}, flags, prefix="")
    return flags


def _collect_flags(properties: Mapping[str, Any], out: list[dict], prefix: str) -> None:
    for name, schema in properties.items():
        if not isinstance(schema, Mapping):
            continue
        path = f"{prefix}.{name}" if prefix else name
        cli = schema.get("x-cli")
        if isinstance(cli, Mapping):
            out.append(
                {
                    "flag": cli.get("flag"),
                    "alias": cli.get("alias"),
                    "path": path,
                    "type": schema.get("type"),
                    "enum": list(schema["enum"]) if "enum" in schema else None,
                    "default": schema.get("default"),
                    "description": schema.get("description") or cli.get("description"),
                    "repeat": bool(cli.get("repeat", False)),
                    "invert": bool(cli.get("invert", False)),
                    "enum_map": cli.get("enum_map"),
                }
            )
        if schema.get("type") == "object" and "properties" in schema:
            _collect_flags(schema["properties"], out, prefix=path)


def tool_preset_names(api_def: Mapping[str, Any]) -> list[str]:
    """Return the list of tool preset names defined for this API."""
    presets = api_def.get("tool_presets") or {}
    return sorted(presets.keys())
