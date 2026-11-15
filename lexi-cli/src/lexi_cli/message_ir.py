"""Canonical message intermediate representation (IR).

A message is modeled as an ordered list of **typed content parts**, not a bare
string. Today only :class:`TextPart` exists, so a text-chat message lowers
losslessly to the ``prompt``/``role`` bindings that the API-definition
templates already consume (see ``docs/spec/api-definition-v0.md``). Future
modalities — image, audio, video — add new part types *without* changing this
contract; modeling content as parts now is what makes that additive instead of
a rewrite.

Pure: values in, values out. No I/O, no other Lexi imports.
See ``docs/spec/message-ir-v0.md``.
"""

from __future__ import annotations

from collections.abc import Mapping
from dataclasses import dataclass
from typing import Any

TEXT = "text"
DEFAULT_ROLE = "user"


@dataclass(frozen=True)
class TextPart:
    """A plain-text content part."""

    text: str
    type: str = TEXT

    def to_dict(self) -> dict:
        return {"type": self.type, "text": self.text}


# As modalities are added this becomes a union (TextPart | ImagePart | ...).
ContentPart = TextPart


@dataclass
class Message:
    """A role plus an ordered list of typed content parts."""

    role: str
    content: list  # list[ContentPart]

    def to_dict(self) -> dict:
        return {"role": self.role, "content": [p.to_dict() for p in self.content]}


def text_message(text: str, role: str = DEFAULT_ROLE) -> Message:
    """Build a single-text-part message (the v0 common case)."""
    return Message(role=role, content=[TextPart(text)])


def part_from_dict(data: Mapping[str, Any]) -> ContentPart:
    """Parse a content-part dict. v0 supports only ``type: text``."""
    part_type = data.get("type", TEXT)
    if part_type == TEXT:
        return TextPart(text=data.get("text", ""))
    raise ValueError(f"unsupported content part type {part_type!r} (v0 supports only {TEXT!r})")


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
    """Lower a (text-only) Message to the ``{prompt, role}`` bindings the
    API-definition templates consume.

    This is the degenerate-case bridge: for text chat it reproduces exactly the
    inputs the engine used before the IR existed. Non-text parts are not
    representable in v0 and raise — that is the seam future versions extend.
    """
    for part in message.content:
        if not isinstance(part, TextPart):
            raise NotImplementedError(f"v0 lowering supports only text parts; got {type(part).__name__}")
    return {"role": message.role, "prompt": text_of(message)}
