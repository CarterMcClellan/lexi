"""Run the shared conformance suite against the `lexi` wheel (PyO3 over lexi-core).

This is a THIRD engine behind the same oracle as the Rust
(`lexi-core/tests/conformance.rs`) and Python-`lexi_cli`
(`lexi-cli/tests/test_conformance.py`) runners: the same `conformance/` fixtures,
driven through the compiled Rust core via its Python bindings. Passing here proves
the wheel agrees with both engines.

Requires the wheel installed (`pip install ./lexi-python`).
"""

from __future__ import annotations

import json
from pathlib import Path

import lexi  # the wheel (PyO3 over lexi-core)
import pytest
import yaml

ROOT = Path(__file__).resolve().parents[2]
CONFORMANCE = ROOT / "conformance" / "cases"
APIS_DIR = ROOT / "lexi-core" / "apis"


def _cases(category: str) -> list[Path]:
    return sorted((CONFORMANCE / category).glob("*.json"))


def _load_json(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def _load_api(name: str) -> dict:
    return yaml.safe_load((APIS_DIR / f"{name}.yaml").read_text(encoding="utf-8"))


def _id(path: Path) -> str:
    return path.stem


REQUEST = _cases("request")
RESPONSE = _cases("response")
MESSAGE = _cases("message")
CAPABILITY = _cases("capability")
DEFINITION = _cases("definition")
OPERATION = _cases("operation")
UPLOAD = _cases("upload")
STREAM = _cases("stream")


@pytest.mark.parametrize("path", REQUEST, ids=[_id(p) for p in REQUEST])
def test_request(path: Path) -> None:
    case = _load_json(path)
    api = _load_api(case["api"])
    compose = case.get("compose", {})
    payload = dict(compose.get("prompt_payload") or {})
    content_mapping = api.get("content_mapping")

    msg = compose["message"] if "message" in compose else lexi.normalize_prompt_payload(payload)
    effective = {**payload, **lexi.lower_message(msg, content_mapping)}

    body = lexi.compose_request_body(
        api,
        effective,
        compose.get("overrides"),
        compose.get("tool_presets"),
        compose.get("set_paths"),
        compose.get("unset_paths"),
    )
    assert body == case["expect_body"], f"{case['name']}: body mismatch"


@pytest.mark.parametrize("path", RESPONSE, ids=[_id(p) for p in RESPONSE])
def test_response(path: Path) -> None:
    case = _load_json(path)
    rc = _load_api(case["api"]).get("response", {})
    resp = case["response"]
    expect = case["expect"]

    assert lexi.extract_text(resp, rc.get("text_extract")) == expect["text"], case["name"]

    if "activity" in expect:
        assert lexi.summarize_activity(resp, rc.get("activity")) == expect["activity"], case["name"]

    if "annotations_rendered" in expect:
        items = lexi.find_text_items(resp, rc.get("text_extract"))
        anns = lexi.extract_annotations(resp, rc.get("annotations"), items)
        assert lexi.render_annotations(anns, rc.get("annotations")) == expect["annotations_rendered"], case["name"]

    if "metadata" in expect:
        meta = {k: lexi.get_path(resp, p) for k, p in (rc.get("metadata") or {}).items()}
        assert meta == expect["metadata"], case["name"]

    if "artifacts" in expect:
        assert lexi.extract_artifacts(resp, rc.get("artifacts")) == expect["artifacts"], case["name"]


@pytest.mark.parametrize("path", MESSAGE, ids=[_id(p) for p in MESSAGE])
def test_message(path: Path) -> None:
    case = _load_json(path)
    msg = (
        lexi.normalize_prompt_payload(case["legacy_payload"])
        if "legacy_payload" in case
        else lexi.message_from_dict(case["message"])
    )
    if "expect_message" in case:
        assert msg == case["expect_message"], case["name"]
    if "expect_bindings" in case:
        assert lexi.to_prompt_bindings(msg) == case["expect_bindings"], case["name"]


@pytest.mark.parametrize("path", CAPABILITY, ids=[_id(p) for p in CAPABILITY])
def test_capability(path: Path) -> None:
    case = _load_json(path)
    api = _load_api(case["api"])
    cap = lexi.resolve_capabilities(api, case.get("model_capabilities"))
    if "expect_capabilities" in case:
        assert cap == case["expect_capabilities"], case["name"]
    if "expect_errors" in case:
        assert lexi.validate_message(cap, case["message"]) == case["expect_errors"], case["name"]


@pytest.mark.parametrize("path", OPERATION, ids=[_id(p) for p in OPERATION])
def test_operation(path: Path) -> None:
    case = _load_json(path)
    op_cfg = _load_api(case["api"]).get("operation")
    op = lexi.read_operation(case["response"], op_cfg)
    if "expect_operation" in case:
        assert op == case["expect_operation"], case["name"]
    if "expect_poll_path" in case:
        assert lexi.poll_path(op_cfg, op["id"]) == case["expect_poll_path"], case["name"]


@pytest.mark.parametrize("path", UPLOAD, ids=[_id(p) for p in UPLOAD])
def test_upload(path: Path) -> None:
    case = _load_json(path)
    files_cfg = _load_api(case["api"]).get("files")
    if "expect_upload" in case:
        assert lexi.read_upload(case["response"], files_cfg) == case["expect_upload"], case["name"]
    if "expect_upload_path" in case:
        assert lexi.upload_path(files_cfg) == case["expect_upload_path"], case["name"]


@pytest.mark.parametrize("path", STREAM, ids=[_id(p) for p in STREAM])
def test_stream(path: Path) -> None:
    case = _load_json(path)
    cfg = _load_api(case["api"])["response"]["stream"]
    for i, ev in enumerate(case["events"]):
        got = lexi.classify_event(ev["event"], ev["data"], cfg)
        assert got == case["expect"][i], f"{case['name']}[{i}]"


@pytest.mark.parametrize("path", DEFINITION, ids=[_id(p) for p in DEFINITION])
def test_definition(path: Path) -> None:
    case = _load_json(path)
    definition = case["definition"]
    if "expect_errors" in case:
        assert lexi.validate_definition(definition) == case["expect_errors"], case["name"]
    if "expect_flags" in case:
        assert lexi.cli_flags(definition) == case["expect_flags"], case["name"]
    if "expect_presets" in case:
        assert lexi.tool_preset_names(definition) == case["expect_presets"], case["name"]


def test_preview_binding() -> None:
    # preview_definition stitches validate + flags + presets + a sample compose;
    # each piece is pinned by its own conformance category, so here we just prove
    # the wheel binding assembles them correctly.
    case = _load_json(CONFORMANCE / "definition" / "valid-with-flags-and-presets.json")
    out = lexi.preview_definition(case["definition"], "hi", "m1", "user")
    assert out["valid"] is True
    assert out["errors"] == []
    assert out["flags"] == case["expect_flags"]
    assert out["presets"] == case["expect_presets"]
    assert "body" in out and "body_error" not in out

    bad = lexi.preview_definition({"name": "x"}, "hi", "m1", "user")
    assert bad["valid"] is False
    assert bad["errors"]
    assert "body" not in bad


def test_suite_non_empty() -> None:
    assert REQUEST and RESPONSE and MESSAGE and CAPABILITY and OPERATION and UPLOAD and STREAM
    assert DEFINITION
