"""Tests for the schema-driven API composition engine (api_compose)."""

from __future__ import annotations

import pytest

from lexi_cli.api_compose import (
    cli_flags_for,
    compose_request_body,
    extract_annotations,
    extract_text,
    find_text_items,
    get_path,
    parse_path,
    render_annotations,
    render_template,
    set_path,
    summarize_activity,
    tool_preset_names,
    unset_path,
    validate,
)

# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------


@pytest.fixture
def responses_api():
    """A small but realistic Responses-style API definition."""
    return {
        "name": "responses",
        "request": {
            "method": "POST",
            "path": "/responses",
            "body_schema": {
                "type": "object",
                "required": ["model", "input"],
                "properties": {
                    "model": {"type": "string", "x-bind": {"from": "model"}},
                    "input": {
                        "type": "array",
                        "x-render": {
                            "template": [
                                {
                                    "role": "{{role}}",
                                    "content": [
                                        {"type": "input_text", "text": "{{prompt}}"}
                                    ],
                                }
                            ]
                        },
                    },
                    "temperature": {
                        "type": "number",
                        "minimum": 0,
                        "maximum": 2,
                        "x-cli": {"flag": "--temperature", "alias": "-t"},
                        "x-include": "if_set",
                    },
                    "max_output_tokens": {
                        "type": "integer",
                        "minimum": 1,
                        "x-cli": {"flag": "--max-tokens", "alias": "-m"},
                        "x-include": "if_set",
                    },
                    "stream": {
                        "type": "boolean",
                        "default": False,
                        "x-cli": {"flag": "--stream"},
                        "x-include": "if_truthy",
                    },
                    "reasoning": {
                        "type": "object",
                        "properties": {
                            "effort": {
                                "type": "string",
                                "enum": ["minimal", "low", "medium", "high"],
                            },
                            "summary": {
                                "type": "string",
                                "enum": ["auto", "concise", "detailed"],
                            },
                        },
                        "x-include": "if_any_child_set",
                    },
                    "tools": {
                        "type": "array",
                        "items": {"type": "object"},
                        "x-include": "if_non_empty",
                    },
                    "metadata": {"type": "object", "x-include": "if_set"},
                },
            },
        },
        "tool_presets": {
            "web_search": {"tools": [{"type": "web_search"}]},
            "json_object": {"text": {"format": {"type": "json_object"}}},
        },
    }


# ---------------------------------------------------------------------------
# Path helpers
# ---------------------------------------------------------------------------


class TestParsePath:
    def test_simple(self):
        assert parse_path("a") == ["a"]

    def test_dotted(self):
        assert parse_path("a.b.c") == ["a", "b", "c"]

    def test_indexed(self):
        assert parse_path("messages[0].content") == ["messages", 0, "content"]

    def test_nested_indices(self):
        assert parse_path("a[0][1].b") == ["a", 0, 1, "b"]


class TestGetPath:
    def test_dotted(self):
        assert get_path({"a": {"b": 1}}, "a.b") == 1

    def test_indexed(self):
        assert get_path({"items": [{"x": 9}]}, "items[0].x") == 9

    def test_missing(self):
        assert get_path({"a": 1}, "b") is None
        assert get_path({"a": 1}, "a.b") is None
        assert get_path({"a": [1, 2]}, "a[5]") is None


class TestSetPath:
    def test_simple(self):
        d: dict = {}
        set_path(d, "a", 1)
        assert d == {"a": 1}

    def test_creates_nested_dict(self):
        d: dict = {}
        set_path(d, "a.b.c", 9)
        assert d == {"a": {"b": {"c": 9}}}

    def test_creates_nested_list(self):
        d: dict = {}
        set_path(d, "items[2].name", "x")
        assert d == {"items": [None, None, {"name": "x"}]}

    def test_overwrites_existing(self):
        d: dict = {"a": {"b": 1}}
        set_path(d, "a.b", 2)
        assert d == {"a": {"b": 2}}


class TestUnsetPath:
    def test_remove_key(self):
        d: dict = {"a": {"b": 1, "c": 2}}
        unset_path(d, "a.b")
        assert d == {"a": {"c": 2}}

    def test_missing_is_noop(self):
        d: dict = {"a": 1}
        unset_path(d, "x.y.z")
        assert d == {"a": 1}

    def test_pop_list_index(self):
        d: dict = {"items": [1, 2, 3]}
        unset_path(d, "items[1]")
        assert d == {"items": [1, 3]}


# ---------------------------------------------------------------------------
# Template rendering
# ---------------------------------------------------------------------------


class TestRenderTemplate:
    def test_exact_placeholder_preserves_type(self):
        assert render_template("{{n}}", {"n": 42}) == 42
        assert render_template("{{xs}}", {"xs": [1, 2, 3]}) == [1, 2, 3]

    def test_partial_substitution_stringifies(self):
        out = render_template("hi {{name}}!", {"name": "Lexi"})
        assert out == "hi Lexi!"

    def test_nested_dict(self):
        out = render_template(
            {"role": "{{role}}", "content": "{{prompt}}"},
            {"role": "user", "prompt": "hello"},
        )
        assert out == {"role": "user", "content": "hello"}

    def test_nested_list(self):
        out = render_template(
            [{"role": "{{role}}"}], {"role": "system"}
        )
        assert out == [{"role": "system"}]

    def test_unknown_placeholder_left_intact(self):
        out = render_template("{{missing}}", {"x": 1})
        assert out == "{{missing}}"


# ---------------------------------------------------------------------------
# Validation
# ---------------------------------------------------------------------------


class TestValidate:
    def test_simple_type_ok(self):
        assert validate({"type": "string"}, "abc") == []

    def test_simple_type_fail(self):
        errs = validate({"type": "string"}, 123)
        assert errs and "must be of type" in errs[0]

    def test_bool_is_not_number(self):
        errs = validate({"type": "number"}, True)
        assert errs

    def test_enum_ok(self):
        assert validate({"enum": ["a", "b"]}, "a") == []

    def test_enum_fail(self):
        errs = validate({"enum": ["a", "b"]}, "c")
        assert errs and "must be one of" in errs[0]

    def test_min_max(self):
        s = {"type": "number", "minimum": 0, "maximum": 1}
        assert validate(s, 0.5) == []
        assert "must be >=" in validate(s, -1)[0]
        assert "must be <=" in validate(s, 5)[0]

    def test_required(self):
        s = {"type": "object", "required": ["model"], "properties": {"model": {"type": "string"}}}
        assert validate(s, {"model": "gpt"}) == []
        errs = validate(s, {})
        assert errs and "missing required key" in errs[0]

    def test_oneof_discriminated(self):
        s = {
            "oneOf": [
                {
                    "type": "object",
                    "properties": {"type": {"const": "text"}},
                    "required": ["type"],
                },
                {
                    "type": "object",
                    "properties": {"type": {"const": "json"}},
                    "required": ["type"],
                },
            ]
        }
        assert validate(s, {"type": "text"}) == []
        assert validate(s, {"type": "json"}) == []
        errs = validate(s, {"type": "other"})
        assert errs and "did not match any oneOf" in errs[0]

    def test_oneof_multiple_match(self):
        s = {
            "oneOf": [
                {"type": "object"},
                {"type": "object", "properties": {"x": {"type": "integer"}}},
            ]
        }
        errs = validate(s, {"x": 1})
        assert errs and "matched multiple" in errs[0]


# ---------------------------------------------------------------------------
# Composition
# ---------------------------------------------------------------------------


class TestCompose:
    def test_basic_body(self, responses_api):
        body = compose_request_body(
            responses_api,
            prompt_payload={"prompt": "hi", "role": "user"},
            overrides={"model": "gpt-x"},
        )
        assert body["model"] == "gpt-x"
        assert body["input"] == [
            {"role": "user", "content": [{"type": "input_text", "text": "hi"}]}
        ]
        # if_set: temperature wasn't passed → omitted
        assert "temperature" not in body
        # if_truthy: stream defaults False → omitted
        assert "stream" not in body

    def test_temperature_via_override(self, responses_api):
        body = compose_request_body(
            responses_api,
            prompt_payload={"prompt": "hi", "role": "user"},
            overrides={"model": "m", "temperature": 0.7},
        )
        assert body["temperature"] == 0.7

    def test_stream_truthy(self, responses_api):
        body = compose_request_body(
            responses_api,
            prompt_payload={"prompt": "hi", "role": "user"},
            overrides={"model": "m", "stream": True},
        )
        assert body["stream"] is True

    def test_stream_false_omitted(self, responses_api):
        body = compose_request_body(
            responses_api,
            prompt_payload={"prompt": "hi", "role": "user"},
            overrides={"model": "m", "stream": False},
        )
        assert "stream" not in body

    def test_if_any_child_set(self, responses_api):
        # No reasoning fields set → omitted
        body = compose_request_body(
            responses_api,
            prompt_payload={"prompt": "hi", "role": "user"},
            overrides={"model": "m"},
        )
        assert "reasoning" not in body

        # set via override (whole object)
        body = compose_request_body(
            responses_api,
            prompt_payload={"prompt": "hi", "role": "user"},
            overrides={"model": "m", "reasoning": {"effort": "high"}},
        )
        assert body["reasoning"] == {"effort": "high"}

    def test_if_any_child_set_via_setpath(self, responses_api):
        body = compose_request_body(
            responses_api,
            prompt_payload={"prompt": "hi", "role": "user"},
            overrides={"model": "m"},
            set_paths=[("reasoning.effort", "high")],
        )
        assert body["reasoning"] == {"effort": "high"}

    def test_set_path_creates_nested(self, responses_api):
        body = compose_request_body(
            responses_api,
            prompt_payload={"prompt": "hi", "role": "user"},
            overrides={"model": "m"},
            set_paths=[("metadata.session", "abc")],
        )
        assert body["metadata"] == {"session": "abc"}

    def test_unset_drops_default(self, responses_api):
        # stream default is False but with x-include if_truthy it's already absent,
        # so use unset on an explicitly-present field
        body = compose_request_body(
            responses_api,
            prompt_payload={"prompt": "hi", "role": "user"},
            overrides={"model": "m", "temperature": 0.5},
            unset_paths=["temperature"],
        )
        assert "temperature" not in body

    def test_tool_preset_web_search(self, responses_api):
        body = compose_request_body(
            responses_api,
            prompt_payload={"prompt": "hi", "role": "user"},
            overrides={"model": "m"},
            tool_presets_selected=["web_search"],
        )
        assert body["tools"] == [{"type": "web_search"}]

    def test_tool_preset_unknown_raises(self, responses_api):
        with pytest.raises(KeyError, match="unknown tool preset"):
            compose_request_body(
                responses_api,
                prompt_payload={"prompt": "hi", "role": "user"},
                overrides={"model": "m"},
                tool_presets_selected=["nonexistent"],
            )

    def test_validation_failure_on_temperature(self, responses_api):
        with pytest.raises(ValueError, match="validation failed"):
            compose_request_body(
                responses_api,
                prompt_payload={"prompt": "hi", "role": "user"},
                overrides={"model": "m", "temperature": 99},
            )

    def test_validation_failure_on_missing_model(self, responses_api):
        # No model anywhere → required field missing → fails
        with pytest.raises(ValueError, match="missing required key"):
            compose_request_body(
                responses_api, prompt_payload={"prompt": "hi", "role": "user"}
            )

    def test_skip_validation(self, responses_api):
        body = compose_request_body(
            responses_api,
            prompt_payload={"prompt": "hi", "role": "user"},
            overrides={"model": "m", "temperature": 99},
            skip_validation=True,
        )
        assert body["temperature"] == 99

    def test_overrides_beat_prompt_payload(self, responses_api):
        body = compose_request_body(
            responses_api,
            prompt_payload={"prompt": "hi", "role": "user", "temperature": 0.3},
            overrides={"model": "m", "temperature": 1.1},
        )
        assert body["temperature"] == 1.1

    def test_prompt_payload_used_when_no_override(self, responses_api):
        body = compose_request_body(
            responses_api,
            prompt_payload={"prompt": "hi", "role": "user", "temperature": 0.3},
            overrides={"model": "m"},
        )
        assert body["temperature"] == 0.3

    def test_missing_body_schema_raises(self):
        with pytest.raises(ValueError, match="body_schema"):
            compose_request_body({"request": {}}, prompt_payload={"prompt": "x"})


class TestPresetMerge:
    def test_preset_lists_concat(self, responses_api):
        body = compose_request_body(
            responses_api,
            prompt_payload={"prompt": "hi", "role": "user"},
            overrides={"model": "m", "tools": [{"type": "function", "name": "x"}]},
            tool_presets_selected=["web_search"],
        )
        # Override (function) + preset (web_search) → both present
        assert {"type": "function", "name": "x"} in body["tools"]
        assert {"type": "web_search"} in body["tools"]


class TestCliFlagsFor:
    def test_collects_top_level_flags(self, responses_api):
        flags = cli_flags_for(responses_api)
        names = {f["flag"] for f in flags}
        assert "--temperature" in names
        assert "--max-tokens" in names
        assert "--stream" in names

    def test_includes_path_for_each(self, responses_api):
        flags = {f["flag"]: f for f in cli_flags_for(responses_api)}
        assert flags["--temperature"]["path"] == "temperature"
        assert flags["--temperature"]["alias"] == "-t"
        assert flags["--temperature"]["type"] == "number"


class TestToolPresetNames:
    def test_lists_presets(self, responses_api):
        assert tool_preset_names(responses_api) == ["json_object", "web_search"]

    def test_empty_when_absent(self):
        assert tool_preset_names({"request": {}}) == []


# ---------------------------------------------------------------------------
# extract_text — generic response text extraction
# ---------------------------------------------------------------------------


class TestExtractText:
    def test_simple_text_path(self):
        # Mode A — single path lookup
        cfg = {"text_path": "choices[0].message.content"}
        resp = {"choices": [{"message": {"content": "hello"}}]}
        assert extract_text(resp, cfg) == "hello"

    def test_simple_text_path_missing(self):
        cfg = {"text_path": "choices[0].message.content"}
        assert extract_text({}, cfg) == ""

    def test_walked_responses_with_only_message(self):
        # Mode B — Responses API shape, no tool calls
        cfg = {
            "items_path": "output",
            "item_filter": {"type": "message"},
            "content_path": "content",
            "content_filter": {"type": "output_text"},
            "text_field": "text",
        }
        resp = {
            "output": [
                {
                    "type": "message",
                    "content": [{"type": "output_text", "text": "Hello!"}],
                }
            ]
        }
        assert extract_text(resp, cfg) == "Hello!"

    def test_walked_responses_skips_web_search_call(self):
        # The actual bug: output[0] is a web_search_call; message is at output[1].
        cfg = {
            "items_path": "output",
            "item_filter": {"type": "message"},
            "content_path": "content",
            "content_filter": {"type": "output_text"},
            "text_field": "text",
        }
        resp = {
            "output": [
                {"type": "web_search_call", "id": "ws_1", "status": "completed"},
                {
                    "type": "reasoning",
                    "summary": [{"type": "summary_text", "text": "thinking..."}],
                },
                {
                    "type": "message",
                    "role": "assistant",
                    "content": [
                        {"type": "output_text", "text": "Based on the news, "},
                        {"type": "output_text", "text": "NVDA is up."},
                    ],
                },
            ]
        }
        assert extract_text(resp, cfg) == "Based on the news, NVDA is up."

    def test_walked_anthropic_with_thinking_block(self):
        # Mode B — Anthropic shape, content is a flat list of blocks
        cfg = {
            "items_path": "content",
            "item_filter": {"type": "text"},
            "text_field": "text",
        }
        resp = {
            "content": [
                {"type": "thinking", "thinking": "..."},
                {"type": "tool_use", "id": "tool_1", "name": "x"},
                {"type": "text", "text": "The answer is 42."},
            ]
        }
        assert extract_text(resp, cfg) == "The answer is 42."

    def test_no_match_returns_empty(self):
        cfg = {
            "items_path": "output",
            "item_filter": {"type": "message"},
            "content_path": "content",
        }
        # Only a web_search_call — no message item
        resp = {"output": [{"type": "web_search_call"}]}
        assert extract_text(resp, cfg) == ""

    def test_joiner(self):
        cfg = {
            "items_path": "output",
            "item_filter": {"type": "message"},
            "content_path": "content",
            "content_filter": {"type": "output_text"},
            "joiner": " | ",
        }
        resp = {
            "output": [
                {
                    "type": "message",
                    "content": [
                        {"type": "output_text", "text": "a"},
                        {"type": "output_text", "text": "b"},
                    ],
                }
            ]
        }
        assert extract_text(resp, cfg) == "a | b"

    def test_empty_cfg_returns_empty(self):
        assert extract_text({"output_text": "hi"}, None) == ""
        assert extract_text({"output_text": "hi"}, {}) == ""


# ---------------------------------------------------------------------------
# extract_annotations / render_annotations
# ---------------------------------------------------------------------------


class TestExtractAnnotations:
    def _responses_resp(self):
        return {
            "output": [
                {"type": "web_search_call", "id": "ws_1"},
                {
                    "type": "message",
                    "content": [
                        {
                            "type": "output_text",
                            "text": "Recent NVDA news...",
                            "annotations": [
                                {
                                    "type": "url_citation",
                                    "url": "https://example.com/a",
                                    "title": "Article A",
                                    "start_index": 0,
                                    "end_index": 18,
                                },
                                {
                                    "type": "url_citation",
                                    "url": "https://example.com/b",
                                    "title": "Article B",
                                },
                            ],
                        }
                    ],
                },
            ]
        }

    def _text_extract_cfg(self):
        return {
            "items_path": "output",
            "item_filter": {"type": "message"},
            "content_path": "content",
            "content_filter": {"type": "output_text"},
            "text_field": "text",
        }

    def _annotations_cfg(self):
        return {
            "scope": "text_items",
            "path": "annotations",
            "types": {
                "url_citation": {"format": "[{i}] {url} — {title}"},
                "file_citation": {"format": "[{i}] file:{file_id}"},
            },
        }

    def test_extract_text_items_scope(self):
        resp = self._responses_resp()
        items = find_text_items(resp, self._text_extract_cfg())
        anns = extract_annotations(resp, self._annotations_cfg(), text_items=items)
        assert len(anns) == 2
        assert anns[0]["url"] == "https://example.com/a"
        assert anns[1]["url"] == "https://example.com/b"

    def test_filters_unknown_types(self):
        resp = self._responses_resp()
        # Add an annotation type not in the config
        resp["output"][1]["content"][0]["annotations"].append(
            {"type": "weird_thing", "x": 1}
        )
        items = find_text_items(resp, self._text_extract_cfg())
        anns = extract_annotations(resp, self._annotations_cfg(), text_items=items)
        assert len(anns) == 2  # weird_thing skipped
        assert all(a["type"] == "url_citation" for a in anns)

    def test_empty_when_no_text_items(self):
        # text_items empty -> nothing to scan
        anns = extract_annotations({}, self._annotations_cfg(), text_items=[])
        assert anns == []

    def test_response_root_scope(self):
        resp = {
            "annotations": [
                {"type": "url_citation", "url": "https://r/", "title": "Root"}
            ]
        }
        cfg = {
            "scope": "response_root",
            "path": "annotations",
            "types": {"url_citation": {"format": "[{i}] {url}"}},
        }
        anns = extract_annotations(resp, cfg, text_items=None)
        assert len(anns) == 1

    def test_no_cfg_returns_empty(self):
        assert extract_annotations({"x": 1}, None, text_items=[{"a": [1]}]) == []
        assert extract_annotations({"x": 1}, {}, text_items=[{"a": [1]}]) == []


class TestRenderAnnotations:
    def test_render_with_format_substitution(self):
        anns = [
            {"type": "url_citation", "url": "https://a/", "title": "A"},
            {"type": "url_citation", "url": "https://b/", "title": "B"},
        ]
        cfg = {"types": {"url_citation": {"format": "[{i}] {url} — {title}"}}}
        out = render_annotations(anns, cfg)
        assert out == "[1] https://a/ — A\n[2] https://b/ — B"

    def test_missing_field_renders_empty(self):
        anns = [{"type": "url_citation", "url": "https://a/"}]  # no title
        cfg = {"types": {"url_citation": {"format": "[{i}] {url} — {title}"}}}
        out = render_annotations(anns, cfg)
        assert out == "[1] https://a/ — "

    def test_empty_returns_empty(self):
        assert render_annotations([], {"types": {}}) == ""

    def test_unknown_type_falls_back(self):
        anns = [{"type": "unknown_type", "x": 1}]
        cfg = {"types": {"unknown_type": {}}}  # no format
        out = render_annotations(anns, cfg)
        # Falls back to a default format that includes the raw dict
        assert out.startswith("[1]")


class TestSummarizeActivity:
    def _cfg(self):
        return {
            "items_path": "output",
            "summarize_by_type": {
                "web_search_call":       "{count} web search",
                "file_search_call":      "{count} file search",
                "reasoning":             "reasoning",
                "function_call":         "{count} function call",
            },
        }

    def test_counts_and_orders_per_config(self):
        resp = {
            "output": [
                {"type": "reasoning"},
                {"type": "web_search_call"},
                {"type": "web_search_call"},
                {"type": "message"},
            ]
        }
        # Order follows config insertion order.
        assert summarize_activity(resp, self._cfg()) == "2 web search · reasoning"

    def test_zero_counts_omitted(self):
        resp = {"output": [{"type": "message"}]}
        assert summarize_activity(resp, self._cfg()) == ""

    def test_static_label_no_count_placeholder(self):
        # 'reasoning' label has no {count} — appears once even with multiple items.
        resp = {"output": [{"type": "reasoning"}, {"type": "reasoning"}]}
        assert summarize_activity(resp, self._cfg()) == "reasoning"

    def test_function_call_with_count(self):
        resp = {
            "output": [
                {"type": "function_call"},
                {"type": "function_call"},
                {"type": "function_call"},
            ]
        }
        assert summarize_activity(resp, self._cfg()) == "3 function call"

    def test_no_cfg_returns_empty(self):
        assert summarize_activity({"output": []}, None) == ""
        assert summarize_activity({"output": []}, {}) == ""

    def test_missing_items_path_returns_empty(self):
        # items_path resolves to None / not a list
        assert summarize_activity({}, self._cfg()) == ""


class TestFindTextItems:
    def test_returns_matched_content_nodes(self):
        cfg = {
            "items_path": "output",
            "item_filter": {"type": "message"},
            "content_path": "content",
            "content_filter": {"type": "output_text"},
        }
        resp = {
            "output": [
                {"type": "web_search_call"},
                {
                    "type": "message",
                    "content": [
                        {"type": "output_text", "text": "x", "annotations": [1]},
                        {"type": "output_text", "text": "y", "annotations": [2]},
                    ],
                },
            ]
        }
        items = find_text_items(resp, cfg)
        assert [i["text"] for i in items] == ["x", "y"]
        assert [i["annotations"] for i in items] == [[1], [2]]

    def test_mode_a_returns_empty(self):
        # Mode A doesn't enumerate items
        assert find_text_items({"x": "y"}, {"text_path": "x"}) == []
