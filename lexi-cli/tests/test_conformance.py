"""Run the language-neutral conformance suite against the Python engine.

The fixtures under ``/conformance`` pin the behavior of the API-definition
engine. They are shared with future non-Python implementations (e.g. the Rust
``lexi-core``), so this runner treats them as the source of truth and simply
drives ``api_compose`` with each case's inputs.

See ``conformance/README.md`` and ``docs/spec/api-definition-v0.md``.
"""

from __future__ import annotations

import json
from pathlib import Path

import pytest
import yaml

import lexi_cli
from lexi_cli import api_compose, capabilities, message_ir, operations, uploads

CONFORMANCE_DIR = Path(__file__).resolve().parents[2] / "conformance"
REQUEST_CASES = sorted((CONFORMANCE_DIR / "cases" / "request").glob("*.json"))
RESPONSE_CASES = sorted((CONFORMANCE_DIR / "cases" / "response").glob("*.json"))
MESSAGE_CASES = sorted((CONFORMANCE_DIR / "cases" / "message").glob("*.json"))
CAPABILITY_CASES = sorted((CONFORMANCE_DIR / "cases" / "capability").glob("*.json"))
OPERATION_CASES = sorted((CONFORMANCE_DIR / "cases" / "operation").glob("*.json"))
UPLOAD_CASES = sorted((CONFORMANCE_DIR / "cases" / "upload").glob("*.json"))
APIS_DIR = Path(lexi_cli.__file__).resolve().parent / "apis"


def _load_json(path: Path) -> dict:
    # Explicit UTF-8: fixtures contain non-ASCII (em dash, guillemets); Windows
    # would otherwise decode with the locale codepage (cp1252) and fail.
    return json.loads(path.read_text(encoding="utf-8"))


def _load_api(name: str) -> dict:
    """Resolve an API definition by name from the packaged apis/ directory."""
    path = APIS_DIR / f"{name}.yaml"
    assert path.exists(), f"API definition {name!r} not found at {path}"
    # Explicit UTF-8: API defs contain emoji (stream.activity_events) whose
    # bytes are undefined in Windows' cp1252 default. Rust's read_to_string
    # is always UTF-8, which is why only the Python runner hit this.
    return yaml.safe_load(path.read_text(encoding="utf-8"))


def _case_id(path: Path) -> str:
    return path.stem


# ---------------------------------------------------------------------------
# Request composition
# ---------------------------------------------------------------------------


@pytest.mark.parametrize("case_path", REQUEST_CASES, ids=[_case_id(p) for p in REQUEST_CASES])
def test_request_composition(case_path: Path) -> None:
    case = _load_json(case_path)
    api_def = _load_api(case["api"])
    compose = case.get("compose", {})

    # Mirror respond_with_provider: normalize the message (or a canonical
    # `message` with image parts) and lower its content via content_mapping.
    payload = dict(compose.get("prompt_payload") or {})
    content_mapping = api_def.get("content_mapping")
    if "message" in compose:
        msg = message_ir.message_from_dict(compose["message"])
    else:
        msg = message_ir.normalize_prompt_payload(payload)
    effective = {**payload, **message_ir.lower_message(msg, content_mapping)}

    set_paths = [tuple(pair) for pair in compose.get("set_paths", [])]

    body = api_compose.compose_request_body(
        api_def,
        prompt_payload=effective,
        overrides=compose.get("overrides"),
        tool_presets_selected=compose.get("tool_presets"),
        set_paths=set_paths,
        unset_paths=compose.get("unset_paths"),
    )

    assert body == case["expect_body"], f"{case['name']}: composed body mismatch"


# ---------------------------------------------------------------------------
# Response extraction
# ---------------------------------------------------------------------------


@pytest.mark.parametrize("case_path", RESPONSE_CASES, ids=[_case_id(p) for p in RESPONSE_CASES])
def test_response_extraction(case_path: Path) -> None:
    case = _load_json(case_path)
    api_def = _load_api(case["api"])
    resp_cfg = api_def.get("response", {})
    response = case["response"]
    expect = case["expect"]

    text_extract = resp_cfg.get("text_extract")
    text = api_compose.extract_text(response, text_extract)
    assert text == expect["text"], f"{case['name']}: text mismatch"

    if "activity" in expect:
        activity = api_compose.summarize_activity(response, resp_cfg.get("activity"))
        assert activity == expect["activity"], f"{case['name']}: activity mismatch"

    if "annotations_rendered" in expect:
        annotations_cfg = resp_cfg.get("annotations")
        text_items = api_compose.find_text_items(response, text_extract)
        anns = api_compose.extract_annotations(response, annotations_cfg, text_items)
        rendered = api_compose.render_annotations(anns, annotations_cfg)
        assert rendered == expect["annotations_rendered"], f"{case['name']}: annotations mismatch"

    if "metadata" in expect:
        meta_map = resp_cfg.get("metadata") or {}
        meta = {k: api_compose.get_path(response, path) for k, path in meta_map.items()}
        assert meta == expect["metadata"], f"{case['name']}: metadata mismatch"

    if "artifacts" in expect:
        artifacts = api_compose.extract_artifacts(response, resp_cfg.get("artifacts"))
        assert artifacts == expect["artifacts"], f"{case['name']}: artifacts mismatch"


# ---------------------------------------------------------------------------
# Message IR (canonical message -> lowered bindings)
# ---------------------------------------------------------------------------


@pytest.mark.parametrize("case_path", MESSAGE_CASES, ids=[_case_id(p) for p in MESSAGE_CASES])
def test_message_ir(case_path: Path) -> None:
    case = _load_json(case_path)

    if "legacy_payload" in case:
        msg = message_ir.normalize_prompt_payload(case["legacy_payload"])
    else:
        msg = message_ir.message_from_dict(case["message"])

    if "expect_message" in case:
        assert msg.to_dict() == case["expect_message"], f"{case['name']}: message mismatch"

    if "expect_bindings" in case:
        assert message_ir.to_prompt_bindings(msg) == case["expect_bindings"], f"{case['name']}: bindings mismatch"


# ---------------------------------------------------------------------------
# Capability registry (resolution + message validation)
# ---------------------------------------------------------------------------


@pytest.mark.parametrize("case_path", CAPABILITY_CASES, ids=[_case_id(p) for p in CAPABILITY_CASES])
def test_capabilities(case_path: Path) -> None:
    case = _load_json(case_path)
    api_def = _load_api(case["api"])
    cap = capabilities.resolve_capabilities(api_def, case.get("model_capabilities"))

    if "expect_capabilities" in case:
        assert cap == case["expect_capabilities"], f"{case['name']}: capabilities mismatch"

    if "expect_errors" in case:
        msg = message_ir.message_from_dict(case["message"])
        errors = capabilities.validate_message(cap, msg)
        assert errors == case["expect_errors"], f"{case['name']}: validation mismatch"


# ---------------------------------------------------------------------------
# Async operation lifecycle (read handle/status, classify, poll path)
# ---------------------------------------------------------------------------


@pytest.mark.parametrize(
    "case_path", OPERATION_CASES, ids=[_case_id(p) for p in OPERATION_CASES]
)
def test_operations(case_path: Path) -> None:
    case = _load_json(case_path)
    api_def = _load_api(case["api"])
    op_cfg = api_def.get("operation")

    op = operations.read_operation(case["response"], op_cfg)
    if "expect_operation" in case:
        assert op == case["expect_operation"], f"{case['name']}: operation mismatch"

    if "expect_poll_path" in case:
        path = operations.poll_path(op_cfg, op["id"])
        assert path == case["expect_poll_path"], f"{case['name']}: poll path mismatch"


# ---------------------------------------------------------------------------
# Files-API uploads (read handle + upload path)
# ---------------------------------------------------------------------------


@pytest.mark.parametrize("case_path", UPLOAD_CASES, ids=[_case_id(p) for p in UPLOAD_CASES])
def test_uploads(case_path: Path) -> None:
    case = _load_json(case_path)
    api_def = _load_api(case["api"])
    files_cfg = api_def.get("files")

    if "expect_upload" in case:
        up = uploads.read_upload(case["response"], files_cfg)
        assert up == case["expect_upload"], f"{case['name']}: upload handle mismatch"

    if "expect_upload_path" in case:
        path = uploads.upload_path(files_cfg)
        assert path == case["expect_upload_path"], f"{case['name']}: upload path mismatch"


def test_suite_is_non_empty() -> None:
    """Guard against a silently-empty suite (e.g. wrong path)."""
    assert REQUEST_CASES, "no request conformance cases discovered"
    assert RESPONSE_CASES, "no response conformance cases discovered"
    assert MESSAGE_CASES, "no message conformance cases discovered"
    assert CAPABILITY_CASES, "no capability conformance cases discovered"
    assert OPERATION_CASES, "no operation conformance cases discovered"
    assert UPLOAD_CASES, "no upload conformance cases discovered"
