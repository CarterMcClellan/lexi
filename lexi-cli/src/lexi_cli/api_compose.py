"""CLI-side helpers retained after the engine moved to Rust (`lexi` wheel).

The schema-driven engine (request composition, response extraction, template
rendering, validation, message IR) now lives in `lexi-core` and is reached from
Python via the `lexi` wheel. This module keeps only the helpers the CLI needs
that are *not* part of that engine surface:

* ``parse_path`` / ``set_path`` / ``unset_path`` — dotted/bracketed path edits
  used by the CLI's ``--set`` / ``--unset`` prompt-payload manipulation.

(Definition validation and CLI-flag/preset introspection moved to the engine:
``lexi.validate_definition`` / ``lexi.cli_flags`` / ``lexi.tool_preset_names`` /
``lexi.preview_definition``.)

The engine's behavior is pinned by the shared conformance suite, run against the
Rust core (``lexi-core/tests``) and its wheel (``lexi-python/tests``).
"""

from __future__ import annotations

import re
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
