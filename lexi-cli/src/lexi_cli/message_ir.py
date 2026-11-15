"""Canonical message intermediate representation (IR).

A message is modeled as an ordered list of **typed content parts**, not a bare
string. v0 had only :class:`TextPart`; Phase I adds :class:`ImagePart`. Content
is lowered to a provider's wire shape via that provider's ``content_mapping``
(see ``docs/spec/message-ir-v0.md`` and ``docs/spec/api-definition-v0.md``), so
text chat stays byte-identical while image input is purely additive.

Pure: values in, values out. The only Lexi dependency is the pure template
renderer in :mod:`lexi_cli.api_compose`.
"""

from __future__ import annotations

from collections.abc import Mapping
from dataclasses import dataclass
from typing import Any, Union

from lexi_cli.api_compose import render_template

TEXT = "text"
IMAGE = "image"
DEFAULT_ROLE = "user"


@dataclass(frozen=True)
class TextPart:
    """A plain-text content part."""

    text: str
    type: str = TEXT

    def to_dict(self) -> dict:
        return {"type": self.type, "text": self.text}

    def render_vars(self) -> dict:
        return {"text": self.text}


@dataclass(frozen=True)
class ImagePart:
    """An image content part referenced by URL (v0.1: URL sources only)."""

    url: str
    media_type: str | None = None
    type: str = IMAGE

    def to_dict(self) -> dict:
        d: dict[str, Any] = {"type": self.type, "url": self.url}
        if self.media_type is not None:
            d["media_type"] = self.media_type
        return d

    def render_vars(self) -> dict:
        return {"url": self.url, "media_type": self.media_type or ""}


ContentPart = Union[TextPart, ImagePart]


@dataclass
class Message:
    """A role plus an ordered list of typed content parts."""

    role: str
    content: list  # list[ContentPart]

    def to_dict(self) -> dict:
        return {"role": self.role, "content": [p.to_dict() for p in self.content]}


def text_message(text: str, role: str = DEFAULT_ROLE) -> Message:
    """Build a single-text-part message (the common case)."""
    return Message(role=role, content=[TextPart(text)])


def part_from_dict(data: Mapping[str, Any]) -> ContentPart:
    """Parse a content-part dict (``type: text`` or ``type: image``)."""
    part_type = data.get("type", TEXT)
    if part_type == TEXT:
        return TextPart(text=data.get("text", ""))
    if part_type == IMAGE:
        return ImagePart(url=data.get("url", ""), media_type=data.get("media_type"))
    raise ValueError(f"unsupported content part type {part_type!r} (supported: {TEXT!r}, {IMAGE!r})")


def message_from_dict(data: Mapping[str, Any]) -> Message:
    """Parse a canonical message dict.

    ``content`` may be a list of part dicts, a bare string (sugar for a single
    text part), or absent (empty text part).
    """
    role = data.get("role") or DEFAULT_ROLE
    content = data.get("content")
    if isinstance(content, str):
        parts: list = [TextPart(content)]
    elif isinstance(content, list):
        parts = [part_from_dict(p) if isinstance(p, Mapping) else TextPart(str(p)) for p in content]
    elif content is None:
        parts = [TextPart("")]
    else:
        parts = [TextPart(str(content))]
    return Message(role=role, content=parts)


def normalize_prompt_payload(payload: Mapping[str, Any]) -> Message:
    """Upgrade a legacy prompt payload (``{prompt, role, ...}``) into a Message."""
    return text_message(
        text=payload.get("prompt") or "",
        role=payload.get("role") or DEFAULT_ROLE,
    )


def text_of(message: Message) -> str:
    """Concatenate the text of all text parts (joiner ``""``)."""
    return "".join(p.text for p in message.content if isinstance(p, TextPart))


def to_prompt_bindings(message: Message) -> dict:
    """Lower a (text-only) Message to ``{prompt, role}`` bindings (no provider
    mapping). The degenerate-case bridge from Phase 0; see :func:`lower_message`
    for the provider-aware, multimodal version."""
    for part in message.content:
        if not isinstance(part, TextPart):
            raise NotImplementedError(f"to_prompt_bindings supports only text parts; got {type(part).__name__}")
    return {"role": message.role, "prompt": text_of(message)}


def lower_content_part(part: ContentPart, parts_mapping: Mapping[str, Any]) -> Any:
    """Render one content part into a provider's wire shape via its template."""
    spec = parts_mapping.get(part.type)
    if not isinstance(spec, Mapping) or "template" not in spec:
        raise ValueError(f"no content mapping for part type {part.type!r}")
    return render_template(spec["template"], part.render_vars())


def lower_message(message: Message, content_mapping: Mapping[str, Any] | None) -> dict:
    """Lower a Message to template bindings, provider-aware.

    Returns ``{role, content, prompt}`` where ``content`` is the provider's
    per-message content (a string when ``text_only_as_string`` and the message
    is a single text part, else a list of lowered parts). ``prompt`` is retained
    for backward compatibility with text-only ``{{prompt}}`` templates.

    When ``content_mapping`` is absent (e.g. a custom API def without one), falls
    back to the legacy ``{role, prompt}`` bindings.
    """
    if not content_mapping:
        return {"role": message.role, "prompt": text_of(message)}

    parts_mapping = content_mapping.get("parts") or {}
    text_only_as_string = bool(content_mapping.get("text_only_as_string", False))

    if text_only_as_string and len(message.content) == 1 and isinstance(message.content[0], TextPart):
        content: Any = message.content[0].text
    else:
        content = [lower_content_part(p, parts_mapping) for p in message.content]

    return {"role": message.role, "content": content, "prompt": text_of(message)}
