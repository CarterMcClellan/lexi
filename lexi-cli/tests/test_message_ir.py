"""Tests for the canonical message IR (message_ir)."""

from __future__ import annotations

import pytest

from lexi_cli import message_ir
from lexi_cli.message_ir import Message, TextPart


def test_text_message_builds_single_part():
    msg = message_ir.text_message("Hello", role="user")
    assert msg == Message(role="user", content=[TextPart("Hello")])
    assert msg.to_dict() == {
        "role": "user",
        "content": [{"type": "text", "text": "Hello"}],
    }


def test_text_message_default_role():
    assert message_ir.text_message("hi").role == "user"


def test_normalize_legacy_payload():
    msg = message_ir.normalize_prompt_payload({"prompt": "Hi there", "role": "system", "temperature": 0.7})
    assert msg.to_dict() == {
        "role": "system",
        "content": [{"type": "text", "text": "Hi there"}],
    }


def test_normalize_defaults_when_missing():
    msg = message_ir.normalize_prompt_payload({})
    assert msg.role == "user"
    assert message_ir.text_of(msg) == ""


@pytest.mark.parametrize(
    "content,expected_text",
    [
        ("plain string", "plain string"),
        ([{"type": "text", "text": "a"}, {"type": "text", "text": "b"}], "ab"),
        (None, ""),
    ],
)
def test_message_from_dict_content_forms(content, expected_text):
    msg = message_ir.message_from_dict({"role": "user", "content": content})
    assert message_ir.text_of(msg) == expected_text


def test_message_from_dict_defaults_role():
    assert message_ir.message_from_dict({"content": "x"}).role == "user"


def test_part_from_dict_rejects_unknown_type():
    with pytest.raises(ValueError, match="unsupported content part type"):
        message_ir.part_from_dict({"type": "video", "url": "x"})


def test_lowering_roundtrips_text():
    """The degenerate-case bridge: normalize -> lower reproduces the inputs."""
    payload = {"prompt": "Hello", "role": "user"}
    msg = message_ir.normalize_prompt_payload(payload)
    assert message_ir.to_prompt_bindings(msg) == {"role": "user", "prompt": "Hello"}


def test_lowering_concatenates_multiple_text_parts():
    msg = Message(role="user", content=[TextPart("Hello "), TextPart("world")])
    assert message_ir.to_prompt_bindings(msg) == {
        "role": "user",
        "prompt": "Hello world",
    }


class _FakePart:
    """A non-text part stand-in (to_prompt_bindings must refuse it)."""


def test_lowering_refuses_non_text_parts():
    msg = Message(role="user", content=[_FakePart()])
    with pytest.raises(NotImplementedError):
        message_ir.to_prompt_bindings(msg)


# --- multimodal content (Phase I) ------------------------------------------


def test_image_part_parses_and_roundtrips():
    part = message_ir.part_from_dict({"type": "image", "url": "https://x/c.jpg"})
    assert part == message_ir.ImagePart(url="https://x/c.jpg")
    assert part.to_dict() == {"type": "image", "url": "https://x/c.jpg"}


def test_message_from_dict_mixed_parts():
    msg = message_ir.message_from_dict(
        {
            "role": "user",
            "content": [
                {"type": "text", "text": "Describe"},
                {"type": "image", "url": "https://x/c.jpg"},
            ],
        }
    )
    assert msg.content == [
        message_ir.TextPart("Describe"),
        message_ir.ImagePart(url="https://x/c.jpg"),
    ]


# content_mapping fragments matching the shipped API defs.
_ANTHROPIC_MAP = {
    "text_only_as_string": False,
    "parts": {
        "text": {"template": {"type": "text", "text": "{{text}}"}},
        "image": {"template": {"type": "image", "source": {"type": "url", "url": "{{url}}"}}},
    },
}
_CHAT_MAP = {
    "text_only_as_string": True,
    "parts": {
        "text": {"template": {"type": "text", "text": "{{text}}"}},
        "image": {"template": {"type": "image_url", "image_url": {"url": "{{url}}"}}},
    },
}


def test_lower_message_text_only_as_string():
    msg = message_ir.text_message("Hi", role="user")
    assert message_ir.lower_message(msg, _CHAT_MAP) == {
        "role": "user",
        "content": "Hi",
        "prompt": "Hi",
    }


def test_lower_message_text_as_array():
    msg = message_ir.text_message("Hi", role="user")
    assert message_ir.lower_message(msg, _ANTHROPIC_MAP)["content"] == [{"type": "text", "text": "Hi"}]


def test_lower_message_with_image():
    msg = Message(
        role="user",
        content=[message_ir.TextPart("Describe"), message_ir.ImagePart("https://x/c.jpg")],
    )
    assert message_ir.lower_message(msg, _ANTHROPIC_MAP)["content"] == [
        {"type": "text", "text": "Describe"},
        {"type": "image", "source": {"type": "url", "url": "https://x/c.jpg"}},
    ]


def test_lower_message_without_mapping_falls_back_to_prompt():
    msg = message_ir.text_message("Hi", role="user")
    assert message_ir.lower_message(msg, None) == {"role": "user", "prompt": "Hi"}
