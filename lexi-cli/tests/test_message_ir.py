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
        message_ir.part_from_dict({"type": "image", "source": {}})


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
    """A non-text part stand-in (v0 lowering must refuse it)."""


def test_lowering_refuses_non_text_parts():
    msg = Message(role="user", content=[_FakePart()])
    with pytest.raises(NotImplementedError):
        message_ir.to_prompt_bindings(msg)
