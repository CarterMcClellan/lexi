"""Tests for typed output artifact extraction (api_compose.extract_artifacts)."""

from __future__ import annotations

from lexi_cli.api_compose import extract_artifacts

_CFG = {
    "items_path": "data",
    "type": "image",
    "fields": {"url": "url", "b64": "b64_json", "revised_prompt": "revised_prompt"},
}


def test_none_cfg_returns_empty():
    assert extract_artifacts({"data": [{"url": "x"}]}, None) == []


def test_missing_items_returns_empty():
    assert extract_artifacts({}, _CFG) == []
    assert extract_artifacts({"data": "nope"}, _CFG) == []


def test_extracts_present_fields_only():
    resp = {"data": [{"url": "https://a.png", "revised_prompt": "p"}, {"b64_json": "QUJD"}]}
    assert extract_artifacts(resp, _CFG) == [
        {"type": "image", "url": "https://a.png", "revised_prompt": "p"},
        {"type": "image", "b64": "QUJD"},
    ]


def test_default_type_is_file():
    cfg = {"items_path": "data", "fields": {"url": "url"}}
    assert extract_artifacts({"data": [{"url": "x"}]}, cfg) == [{"type": "file", "url": "x"}]
