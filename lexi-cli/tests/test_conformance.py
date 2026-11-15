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
from lexi_cli import api_compose

CONFORMANCE_DIR = Path(__file__).resolve().parents[2] / "conformance"
REQUEST_CASES = sorted((CONFORMANCE_DIR / "cases" / "request").glob("*.json"))
RESPONSE_CASES = sorted((CONFORMANCE_DIR / "cases" / "response").glob("*.json"))
APIS_DIR = Path(lexi_cli.__file__).resolve().parent / "apis"


def _load_json(path: Path) -> dict:
    return json.loads(path.read_text())


def _load_api(name: str) -> dict:
    """Resolve an API definition by name from the packaged apis/ directory."""
    path = APIS_DIR / f"{name}.yaml"
    assert path.exists(), f"API definition {name!r} not found at {path}"
    return yaml.safe_load(path.read_text())


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

    set_paths = [tuple(pair) for pair in compose.get("set_paths", [])]

    body = api_compose.compose_request_body(
        api_def,
        prompt_payload=compose.get("prompt_payload"),
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
        assert rendered == expect["annotations_rendered"], (
            f"{case['name']}: annotations mismatch"
        )

    if "metadata" in expect:
        meta_map = resp_cfg.get("metadata") or {}
        meta = {k: api_compose.get_path(response, path) for k, path in meta_map.items()}
        assert meta == expect["metadata"], f"{case['name']}: metadata mismatch"


def test_suite_is_non_empty() -> None:
    """Guard against a silently-empty suite (e.g. wrong path)."""
    assert REQUEST_CASES, "no request conformance cases discovered"
    assert RESPONSE_CASES, "no response conformance cases discovered"
