"""Schema-driven request body composition for the Lexi API abstraction.

API definitions describe the request body as a JSON Schema (under
``request.body_schema``) with ``x-*`` extensions:

* ``x-bind: { from: <var> }`` — value comes from a binding (model, role, prompt, ...).
* ``x-render: { template: <any> }`` — value is rendered from a template with ``{{var}}`` substitution.
* ``x-cli: { flag, alias, repeat, invert, enum_map, description }`` — exposes the property as a CLI flag.
* ``x-include: always | if_set | if_truthy | if_non_empty | if_any_child_set`` — when to include the field.

Optional ``tool_presets:`` block at the API-def root maps preset names to body fragments
that get deep-merged into the body when selected.

The composer is pure: dicts in, dict out. No I/O, no Lexi imports.
"""

from __future__ import annotations

import copy
import re
from collections.abc import Iterable, Mapping
from typing import Any

# ---------------------------------------------------------------------------
# Path helpers (dotted/bracketed access, e.g. "reasoning.effort", "messages[0].content")
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


def get_path(obj: Any, path: str) -> Any:
    """Walk a path expression and return the value, or ``None`` on any miss."""
    cur: Any = obj
    for seg in parse_path(path):
        if cur is None:
            return None
        if isinstance(seg, int):
            if isinstance(cur, list) and 0 <= seg < len(cur):
                cur = cur[seg]
            else:
                return None
        else:
            if isinstance(cur, dict):
                cur = cur.get(seg)
            else:
                return None
    return cur


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
# Template rendering (preserves type when a value is exactly "{{var}}")
# ---------------------------------------------------------------------------

_PLACEHOLDER = re.compile(r"\{\{(.+?)\}\}")


def render_template(template: Any, variables: Mapping[str, Any]) -> Any:
    """Recursively substitute ``{{var}}`` placeholders.

    A string that is exactly ``"{{var}}"`` is replaced with the *raw* variable
    value (so types like int/list/dict survive). Partial placeholders inside
    a larger string are stringified.
    """
    if isinstance(template, str):
        stripped = template.strip()
        if stripped.startswith("{{") and stripped.endswith("}}"):
            key = stripped[2:-2].strip()
            if key in variables:
                return variables[key]
        return _PLACEHOLDER.sub(
            lambda m: str(variables.get(m.group(1).strip(), m.group(0))), template
        )
    if isinstance(template, dict):
        return {k: render_template(v, variables) for k, v in template.items()}
    if isinstance(template, list):
        return [render_template(item, variables) for item in template]
    return template


# ---------------------------------------------------------------------------
# Validation (small JSON-Schema subset: type, const, enum, min/max, required,
# properties, items, oneOf-by-discriminator)
# ---------------------------------------------------------------------------

_TYPE_MAP = {
    "string": str,
    "integer": int,
    "number": (int, float),
    "boolean": bool,
    "array": list,
    "object": dict,
    "null": type(None),
}


def _matches_type(value: Any, type_name: str) -> bool:
    py_t = _TYPE_MAP.get(type_name)
    if py_t is None:
        return True  # unknown type: be lenient
    if type_name in ("integer", "number") and isinstance(value, bool):
        return False  # JSON Schema: bool is not a number
    return isinstance(value, py_t)


def validate(schema: Mapping[str, Any], instance: Any, *, path: str = "") -> list[str]:
    """Return a list of validation error strings for ``instance`` against ``schema``."""
    errors: list[str] = []

    types = schema.get("type")
    if types is not None:
        type_list = [types] if isinstance(types, str) else list(types)
        if not any(_matches_type(instance, t) for t in type_list):
            errors.append(
                f"{path or 'value'} must be of type {type_list}, got {type(instance).__name__}"
            )
            return errors

    if "const" in schema and instance != schema["const"]:
        errors.append(f"{path or 'value'} must equal {schema['const']!r}")

    if "enum" in schema and instance not in schema["enum"]:
        errors.append(f"{path or 'value'} must be one of {schema['enum']!r}")

    if isinstance(instance, (int, float)) and not isinstance(instance, bool):
        if "minimum" in schema and instance < schema["minimum"]:
            errors.append(f"{path or 'value'} must be >= {schema['minimum']}")
        if "maximum" in schema and instance > schema["maximum"]:
            errors.append(f"{path or 'value'} must be <= {schema['maximum']}")

    if isinstance(instance, dict) and (
        "properties" in schema or "required" in schema
    ):
        for key in schema.get("required", []):
            if key not in instance:
                errors.append(
                    f"{path or 'object'} is missing required key {key!r}"
                )
        props = schema.get("properties", {})
        for key, val in instance.items():
            if key in props:
                child_path = f"{path}.{key}" if path else key
                errors.extend(validate(props[key], val, path=child_path))

    if isinstance(instance, list) and "items" in schema:
        for i, item in enumerate(instance):
            errors.extend(validate(schema["items"], item, path=f"{path}[{i}]"))

    if "oneOf" in schema:
        matches: list[int] = []
        for i, alt in enumerate(schema["oneOf"]):
            if not validate(alt, instance, path=path):
                matches.append(i)
        if not matches:
            errors.append(
                f"{path or 'value'} did not match any oneOf alternative"
            )
        elif len(matches) > 1:
            errors.append(
                f"{path or 'value'} matched multiple oneOf alternatives ({matches})"
            )

    return errors


# ---------------------------------------------------------------------------
# Composition
# ---------------------------------------------------------------------------

_VALID_INCLUDES = {
    "always",
    "if_set",
    "if_truthy",
    "if_non_empty",
    "if_any_child_set",
}


def _default_include(prop_schema: Mapping[str, Any]) -> str:
    """A property with a default or template is included by default; others if_set."""
    if "default" in prop_schema or "x-bind" in prop_schema or "x-render" in prop_schema:
        return "always"
    return "if_set"


def _should_include(value: Any, mode: str) -> bool:
    if mode == "always":
        return True
    if mode == "if_set":
        return value is not None
    if mode == "if_truthy":
        return bool(value)
    if mode == "if_non_empty":
        if value is None:
            return False
        try:
            return len(value) > 0  # type: ignore[arg-type]
        except TypeError:
            return True
    if mode == "if_any_child_set":
        if isinstance(value, dict):
            return any(v is not None for v in value.values())
        if isinstance(value, list):
            return len(value) > 0
        return value is not None
    return value is not None


def _resolve_property(
    name: str,
    schema: Mapping[str, Any],
    prompt_payload: Mapping[str, Any],
    overrides: Mapping[str, Any],
    bindings: Mapping[str, Any],
) -> tuple[Any, bool]:
    """Resolve a property to (value, present). present=False means omit."""
    if "x-bind" in schema:
        var = schema["x-bind"].get("from", name)
        if var in bindings and bindings[var] is not None:
            return bindings[var], True
        if "default" in schema:
            return schema["default"], True
        return None, False

    if "x-render" in schema:
        return render_template(schema["x-render"].get("template"), bindings), True

    if name in overrides and overrides[name] is not None:
        return overrides[name], True
    if name in prompt_payload and prompt_payload[name] is not None:
        return prompt_payload[name], True
    if "default" in schema:
        return schema["default"], True

    return None, False


def _deep_merge(base: dict, overlay: Mapping[str, Any]) -> None:
    """Mutate ``base`` by merging ``overlay`` into it. Lists are concatenated."""
    for k, v in overlay.items():
        if k in base and isinstance(base[k], dict) and isinstance(v, dict):
            _deep_merge(base[k], v)
        elif k in base and isinstance(base[k], list) and isinstance(v, list):
            base[k] = base[k] + list(v)
        else:
            base[k] = copy.deepcopy(v)


def compose_request_body(
    api_def: Mapping[str, Any],
    prompt_payload: Mapping[str, Any] | None = None,
    overrides: Mapping[str, Any] | None = None,
    tool_presets_selected: Iterable[str] | None = None,
    set_paths: Iterable[tuple[str, Any]] | None = None,
    unset_paths: Iterable[str] | None = None,
    *,
    skip_validation: bool = False,
) -> dict:
    """Compose a JSON request body from a schema-driven API definition.

    Precedence (low → high):
      schema default → prompt_payload → overrides → tool presets → set_paths → unset_paths
    """
    req = api_def.get("request") or {}
    body_schema = req.get("body_schema") or {}
    if not body_schema:
        raise ValueError(
            "API definition is missing request.body_schema; cannot compose body."
        )

    pp = prompt_payload or {}
    ov = overrides or {}
    bindings: dict[str, Any] = {**pp, **{k: v for k, v in ov.items() if v is not None}}

    body: dict = {}
    properties = body_schema.get("properties") or {}
    for prop_name, prop_schema in properties.items():
        value, present = _resolve_property(prop_name, prop_schema, pp, ov, bindings)
        if not present:
            continue
        mode = prop_schema.get("x-include", _default_include(prop_schema))
        if mode not in _VALID_INCLUDES:
            raise ValueError(
                f"property {prop_name!r}: invalid x-include {mode!r} (expected one of {_VALID_INCLUDES})"
            )
        if not _should_include(value, mode):
            continue
        body[prop_name] = value

    presets_block = api_def.get("tool_presets") or {}
    for preset_name in tool_presets_selected or []:
        if preset_name not in presets_block:
            raise KeyError(
                f"unknown tool preset {preset_name!r}; "
                f"known presets: {sorted(presets_block.keys())}"
            )
        _deep_merge(body, presets_block[preset_name])

    for path, value in set_paths or []:
        set_path(body, path, value)

    for path in unset_paths or []:
        unset_path(body, path)

    if not skip_validation:
        errors = validate(body_schema, body)
        if errors:
            raise ValueError(
                "Request body validation failed:\n  " + "\n  ".join(errors)
            )

    return body


# ---------------------------------------------------------------------------
# CLI flag introspection (for Phase 3)
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


def _collect_flags(
    properties: Mapping[str, Any], out: list[dict], prefix: str
) -> None:
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


# ---------------------------------------------------------------------------
# Generic response extraction
# ---------------------------------------------------------------------------


def _all_kv_match(item: Any, kv_filter: Mapping[str, Any] | None) -> bool:
    """Return True iff every key/value in ``kv_filter`` matches ``item``.

    Empty/None filter matches everything. Non-dict items only match an empty filter.
    """
    if not kv_filter:
        return True
    if not isinstance(item, dict):
        return False
    return all(item.get(k) == expected for k, expected in kv_filter.items())


def extract_text(response: Any, text_extract_cfg: Mapping[str, Any] | None) -> str:
    """Return the assistant text from ``response`` per ``text_extract`` config.

    Two modes:

    * **Mode A — simple path**: ``text_path`` walks a single dotted/bracketed
      path to a string and returns it.
    * **Mode B — walk-and-filter**: ``items_path`` resolves to a list; each
      item passing ``item_filter`` contributes one or more text chunks
      extracted from ``content_path`` (or the item itself if absent), filtered
      by ``content_filter``, taking ``text_field`` (default ``text``).
      Chunks are concatenated with ``joiner`` (default empty string).

    Returns ``""`` when nothing matches.
    """
    if not text_extract_cfg:
        return ""
    cfg = text_extract_cfg

    # Mode A
    text_path = cfg.get("text_path")
    if text_path:
        v = get_path(response, text_path)
        return v if isinstance(v, str) else ""

    # Mode B
    items_path = cfg.get("items_path")
    if not items_path:
        return ""
    items = get_path(response, items_path)
    if not isinstance(items, list):
        return ""
    item_filter = cfg.get("item_filter") or {}
    content_path = cfg.get("content_path")
    content_filter = cfg.get("content_filter") or {}
    text_field = cfg.get("text_field", "text")
    joiner = cfg.get("joiner", "")

    chunks: list[str] = []
    for item in items:
        if not _all_kv_match(item, item_filter):
            continue
        contents = get_path(item, content_path) if content_path else item
        if contents is None:
            continue
        if not isinstance(contents, list):
            contents = [contents]
        for c in contents:
            if not _all_kv_match(c, content_filter):
                continue
            v = c.get(text_field) if isinstance(c, dict) else None
            if isinstance(v, str):
                chunks.append(v)
    return joiner.join(chunks)


def extract_annotations(
    response: Any,
    annotations_cfg: Mapping[str, Any] | None,
    text_items: list[dict] | None = None,
) -> list[dict]:
    """Return annotations matched by ``annotations_cfg``, in document order.

    The schema:

    .. code-block:: yaml

        annotations:
          scope: text_items | response_root   # default: text_items
          path: annotations                   # relative to each scope source
          types:
            url_citation: { format: "[{i}] {url}" }
            file_citation: { format: "[{i}] file:{file_id}" }

    Only annotations whose ``type`` is listed in ``types`` are returned.
    Each returned dict is the raw annotation, unchanged.
    """
    if not annotations_cfg:
        return []
    cfg = annotations_cfg
    types_cfg = cfg.get("types") or {}
    if not types_cfg:
        return []
    path = cfg.get("path", "annotations")
    scope = cfg.get("scope", "text_items")

    if scope == "response_root":
        sources: list[Any] = [response]
    elif scope == "text_items":
        sources = list(text_items or [])
    else:
        return []

    out: list[dict] = []
    for src in sources:
        annotations = get_path(src, path)
        if not isinstance(annotations, list):
            continue
        for ann in annotations:
            if not isinstance(ann, dict):
                continue
            t = ann.get("type")
            if t not in types_cfg:
                continue
            out.append(ann)
    return out


def render_annotations(
    annotations: list[dict], annotations_cfg: Mapping[str, Any] | None
) -> str:
    """Render annotations using each type's ``format`` template.

    ``{i}`` is auto-numbered starting at 1; other ``{name}`` placeholders are
    substituted from the annotation's fields. Missing fields render as empty
    strings (no crash). Returns an empty string for an empty list.
    """
    if not annotations or not annotations_cfg:
        return ""
    types_cfg = annotations_cfg.get("types") or {}
    lines: list[str] = []
    for i, ann in enumerate(annotations, start=1):
        t = ann.get("type")
        type_cfg = types_cfg.get(t) or {}
        fmt = type_cfg.get("format") or f"[{{i}}] {ann}"
        ctx = _DefaultDict({"i": i, **ann})
        try:
            line = fmt.format_map(ctx)
        except (KeyError, IndexError, ValueError):
            line = f"[{i}] {ann}"
        lines.append(line)
    return "\n".join(lines)


class _DefaultDict(dict):
    """dict subclass that returns '' for missing keys (used by str.format_map)."""

    def __missing__(self, key):
        return ""


def summarize_activity(
    response: Any, activity_cfg: Mapping[str, Any] | None
) -> str:
    """Return a one-line activity summary like ``"1 web search · reasoning"``.

    Walks ``items_path`` (default ``"output"``), counts items by ``type``, then
    for each entry in ``summarize_by_type`` whose count is non-zero, formats the
    label string. ``{count}`` is substituted; labels without ``{count}`` render
    statically. Entries are emitted in the order they appear in the config.
    """
    if not activity_cfg:
        return ""
    cfg = activity_cfg
    types_cfg = cfg.get("summarize_by_type") or {}
    if not types_cfg:
        return ""
    items_path = cfg.get("items_path", "output")
    items = get_path(response, items_path)
    if not isinstance(items, list):
        return ""
    counts: dict[str, int] = {}
    for item in items:
        if isinstance(item, dict):
            t = item.get("type")
            if isinstance(t, str):
                counts[t] = counts.get(t, 0) + 1
    parts: list[str] = []
    for type_name, label in types_cfg.items():
        n = counts.get(type_name, 0)
        if n <= 0:
            continue
        if not isinstance(label, str):
            continue
        parts.append(label.format(count=n))
    return " · ".join(parts)


def find_text_items(
    response: Any, text_extract_cfg: Mapping[str, Any] | None
) -> list[dict]:
    """Return the list of content nodes (Mode B) or empty list (Mode A).

    Used by the annotations extractor in step 2 — text-item-scoped annotations
    walk these same nodes for their ``annotations`` field.
    """
    if not text_extract_cfg:
        return []
    if text_extract_cfg.get("text_path"):
        return []  # Mode A has no enumerable items
    items_path = text_extract_cfg.get("items_path")
    if not items_path:
        return []
    items = get_path(response, items_path)
    if not isinstance(items, list):
        return []
    item_filter = text_extract_cfg.get("item_filter") or {}
    content_path = text_extract_cfg.get("content_path")
    content_filter = text_extract_cfg.get("content_filter") or {}
    matched: list[dict] = []
    for item in items:
        if not _all_kv_match(item, item_filter):
            continue
        contents = get_path(item, content_path) if content_path else item
        if contents is None:
            continue
        if not isinstance(contents, list):
            contents = [contents]
        for c in contents:
            if isinstance(c, dict) and _all_kv_match(c, content_filter):
                matched.append(c)
    return matched
