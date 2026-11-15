"""Tests for the capability registry (capabilities)."""

from __future__ import annotations

from lexi_cli import capabilities, message_ir

_MESSAGES_API = {"capabilities": {"input": ["text", "image"], "output": ["text"]}}


def test_resolve_from_api_def():
    cap = capabilities.resolve_capabilities(_MESSAGES_API)
    assert cap == {"input": ["text", "image"], "output": ["text"], "limits": {}}


def test_resolve_defaults_when_absent():
    assert capabilities.resolve_capabilities({}) == {
        "input": [],
        "output": [],
        "limits": {},
    }


def test_model_override_replaces_modalities_and_merges_limits():
    api = {"capabilities": {"input": ["text", "image"], "output": ["text"], "limits": {"a": 1}}}
    cap = capabilities.resolve_capabilities(api, {"input": ["text"], "limits": {"b": 2}})
    assert cap == {"input": ["text"], "output": ["text"], "limits": {"a": 1, "b": 2}}


def test_supports_queries():
    cap = capabilities.resolve_capabilities(_MESSAGES_API)
    assert capabilities.supports_input(cap, "image")
    assert not capabilities.supports_input(cap, "audio")
    assert capabilities.supports_output(cap, "text")
    assert not capabilities.supports_output(cap, "image")


def test_validate_message_accepts_supported():
    cap = capabilities.resolve_capabilities(_MESSAGES_API)
    msg = message_ir.message_from_dict(
        {"role": "user", "content": [{"type": "text", "text": "hi"}, {"type": "image", "url": "u"}]}
    )
    assert capabilities.validate_message(cap, msg) == []


def test_validate_message_rejects_unsupported():
    cap = capabilities.resolve_capabilities(_MESSAGES_API, {"input": ["text"]})
    msg = message_ir.message_from_dict(
        {"role": "user", "content": [{"type": "text", "text": "hi"}, {"type": "image", "url": "u"}]}
    )
    assert capabilities.validate_message(cap, msg) == ["model does not accept image input"]


def test_validate_message_dedupes_per_modality():
    cap = capabilities.resolve_capabilities(_MESSAGES_API, {"input": ["text"]})
    msg = message_ir.message_from_dict(
        {
            "role": "user",
            "content": [
                {"type": "image", "url": "a"},
                {"type": "image", "url": "b"},
            ],
        }
    )
    assert capabilities.validate_message(cap, msg) == ["model does not accept image input"]
