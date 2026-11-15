"""Tests for the schema-driven CLI surface introduced in Phase 3."""

from __future__ import annotations

import json

import lexi  # cli_flags / tool_preset_names live in the engine wheel now
import pytest

from lexi_cli.cli import (
    _coerce_set_value,
    _coerce_typed_value,
    _load_params_file,
    _normalize_flag,
    _parse_respond_flags,
    _parse_set_arg,
    _read_sse_events,
    _stream_sse_response,
    parse_prompt_options,
)


@pytest.fixture
def api_def():
    """Schema-driven API def used to derive flag specs."""
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
                        "x-render": {"template": [{"role": "{{role}}", "text": "{{prompt}}"}]},
                    },
                    "temperature": {
                        "type": "number",
                        "minimum": 0,
                        "maximum": 2,
                        "x-cli": {"flag": "--temperature", "alias": "-t"},
                    },
                    "max_output_tokens": {
                        "type": "integer",
                        "minimum": 1,
                        "x-cli": {"flag": "--max-tokens", "alias": "-m"},
                    },
                    "stream": {
                        "type": "boolean",
                        "default": False,
                        "x-cli": {"flag": "--stream"},
                        "x-include": "if_truthy",
                    },
                    "include": {
                        "type": "array",
                        "items": {"type": "string"},
                        "x-cli": {"flag": "--include", "repeat": True},
                        "x-include": "if_non_empty",
                    },
                    "reasoning": {
                        "type": "object",
                        "properties": {
                            "effort": {
                                "type": "string",
                                "enum": ["minimal", "low", "medium", "high"],
                                "x-cli": {"flag": "--reasoning-effort"},
                            }
                        },
                        "x-include": "if_any_child_set",
                    },
                },
            },
        },
        "tool_presets": {
            "web_search": {"tools": [{"type": "web_search"}]},
            "json_object": {"text": {"format": {"type": "json_object"}}},
        },
        "response": {
            "text_extract": {
                "items_path": "output",
                "item_filter": {"type": "message"},
                "content_path": "content",
                "content_filter": {"type": "output_text"},
                "text_field": "text",
            },
            "annotations": {
                "scope": "text_items",
                "path": "annotations",
                "types": {
                    "url_citation": {"format": "[{i}] {url} — {title}"},
                },
            },
            "activity": {
                "items_path": "output",
                "summarize_by_type": {
                    "web_search_call": "{count} web search",
                    "reasoning": "reasoning",
                },
            },
            "stream": {
                "delta_event": "response.output_text.delta",
                "delta_path": "delta",
                "done_event": "response.completed",
                "final_event": "response.completed",
                "activity_events": {
                    "response.web_search_call.searching": "🔍 searching",
                    "response.web_search_call.completed": "🔍 done",
                },
            },
        },
    }


# ---------------------------------------------------------------------------
# Coercion helpers
# ---------------------------------------------------------------------------


class TestCoerceSetValue:
    def test_int(self):
        assert _coerce_set_value("42") == 42

    def test_float(self):
        assert _coerce_set_value("0.7") == 0.7

    def test_bool(self):
        assert _coerce_set_value("true") is True
        assert _coerce_set_value("False") is False

    def test_null(self):
        assert _coerce_set_value("null") is None

    def test_string_passthrough(self):
        assert _coerce_set_value("hello") == "hello"

    def test_inline_json_array(self):
        assert _coerce_set_value("[1,2,3]") == [1, 2, 3]

    def test_inline_json_object(self):
        assert _coerce_set_value('{"k":"v"}') == {"k": "v"}

    def test_explicit_json_prefix(self):
        assert _coerce_set_value(":json:[1, 2]") == [1, 2]


class TestParseSetArg:
    def test_basic(self):
        assert _parse_set_arg("temperature=0.7") == ("temperature", 0.7)

    def test_dotted(self):
        path, val = _parse_set_arg("reasoning.effort=high")
        assert path == "reasoning.effort"
        assert val == "high"

    def test_string_value_with_equals(self):
        # Only the FIRST '=' is the delimiter
        assert _parse_set_arg("metadata.note=key=value") == ("metadata.note", "key=value")

    def test_missing_equals(self):
        with pytest.raises(ValueError, match="path=value"):
            _parse_set_arg("nothere")


class TestCoerceTypedValue:
    def test_integer(self):
        assert _coerce_typed_value("100", "integer", None, None) == 100

    def test_integer_invalid(self):
        with pytest.raises(ValueError):
            _coerce_typed_value("abc", "integer", None, None)

    def test_number(self):
        assert _coerce_typed_value("0.5", "number", None, None) == 0.5

    def test_boolean_true(self):
        assert _coerce_typed_value("true", "boolean", None, None) is True

    def test_boolean_invalid(self):
        with pytest.raises(ValueError):
            _coerce_typed_value("maybe", "boolean", None, None)

    def test_object(self):
        assert _coerce_typed_value('{"a":1}', "object", None, None) == {"a": 1}

    def test_array(self):
        assert _coerce_typed_value("[1,2]", "array", None, None) == [1, 2]

    def test_enum_map(self):
        assert _coerce_typed_value("json", "string", None, {"json": {"format": "json_object"}}) == {
            "format": "json_object"
        }

    def test_enum_validation(self):
        with pytest.raises(ValueError, match="not in enum"):
            _coerce_typed_value("xtreme", "string", ["minimal", "low", "high"], None)


class TestNormalizeFlag:
    def test_underscore_to_hyphen(self):
        assert _normalize_flag("--max_tokens") == "--max-tokens"

    def test_passthrough_for_short(self):
        assert _normalize_flag("-r") == "-r"


# ---------------------------------------------------------------------------
# Schema-driven flag parser
# ---------------------------------------------------------------------------


class TestParseRespondFlags:
    def _parse(self, api_def, tokens):
        specs = lexi.cli_flags(api_def)
        presets = lexi.tool_preset_names(api_def)
        return _parse_respond_flags(specs, presets, tokens, start=0)

    def test_temperature(self, api_def):
        parsed, err = self._parse(api_def, ["--temperature", "0.5"])
        assert err is None
        assert parsed["set_paths"] == [("temperature", 0.5)]

    def test_temperature_alias(self, api_def):
        parsed, err = self._parse(api_def, ["-t", "1.0"])
        assert err is None
        assert parsed["set_paths"] == [("temperature", 1.0)]

    def test_max_tokens_underscore_normalized(self, api_def):
        parsed, err = self._parse(api_def, ["--max_tokens", "100"])
        assert err is None
        assert parsed["set_paths"] == [("max_output_tokens", 100)]

    def test_boolean_switch(self, api_def):
        parsed, err = self._parse(api_def, ["--stream"])
        assert err is None
        assert parsed["set_paths"] == [("stream", True)]

    def test_repeatable_flag(self, api_def):
        parsed, err = self._parse(api_def, ["--include", "a", "--include", "b"])
        assert err is None
        # set_paths has one entry, accumulating both values
        assert parsed["set_paths"] == [("include", ["a", "b"])]

    def test_set_with_dotted_path(self, api_def):
        parsed, err = self._parse(api_def, ["--set", "reasoning.effort=high"])
        assert err is None
        assert parsed["set_paths"] == [("reasoning.effort", "high")]

    def test_set_with_json_value(self, api_def):
        parsed, err = self._parse(
            api_def, ["--set", "metadata=:json:{\"k\":\"v\"}"]
        )
        assert err is None
        assert parsed["set_paths"] == [("metadata", {"k": "v"})]

    def test_unset(self, api_def):
        parsed, err = self._parse(api_def, ["--unset", "temperature"])
        assert err is None
        assert parsed["unset_paths"] == ["temperature"]

    def test_tool_single(self, api_def):
        parsed, err = self._parse(api_def, ["--tool", "web_search"])
        assert err is None
        assert parsed["tool_presets_selected"] == ["web_search"]

    def test_tool_csv(self, api_def):
        parsed, err = self._parse(api_def, ["--tool", "web_search,json_object"])
        assert err is None
        assert parsed["tool_presets_selected"] == ["web_search", "json_object"]

    def test_tool_unknown(self, api_def):
        _, err = self._parse(api_def, ["--tool", "nonexistent"])
        assert err and "unknown preset" in err

    def test_model(self, api_def):
        parsed, err = self._parse(api_def, ["--model", "gpt-x"])
        assert err is None
        assert parsed["model_override"] == "gpt-x"

    def test_raw_and_verbose(self, api_def):
        parsed, err = self._parse(api_def, ["-r", "-v"])
        assert err is None
        assert parsed["raw"] is True
        assert parsed["verbose"] is True

    def test_unknown_flag(self, api_def):
        _, err = self._parse(api_def, ["--bogus", "1"])
        assert err and "Unknown option" in err

    def test_missing_value(self, api_def):
        _, err = self._parse(api_def, ["--temperature"])
        assert err and "requires a value" in err

    def test_typed_validation_error(self, api_def):
        _, err = self._parse(api_def, ["--temperature", "not-a-number"])
        assert err and "expected number" in err

    def test_reasoning_effort_dotted_path(self, api_def):
        # The --reasoning-effort flag's path is reasoning.effort (nested)
        parsed, err = self._parse(api_def, ["--reasoning-effort", "high"])
        assert err is None
        assert ("reasoning.effort", "high") in parsed["set_paths"]

    def test_params_flag(self, api_def):
        parsed, err = self._parse(api_def, ["--params", "/tmp/x.yaml"])
        assert err is None
        assert parsed["params_file"] == "/tmp/x.yaml"

    def test_no_validate(self, api_def):
        parsed, err = self._parse(api_def, ["--no-validate"])
        assert err is None
        assert parsed["no_validate"] is True


# ---------------------------------------------------------------------------
# --params file loader
# ---------------------------------------------------------------------------


class TestLoadParamsFile:
    def test_yaml(self, tmp_path):
        p = tmp_path / "x.yaml"
        p.write_text("temperature: 1.2\nreasoning:\n  effort: high\n")
        data, err = _load_params_file(str(p))
        assert err is None
        assert data == {"temperature": 1.2, "reasoning": {"effort": "high"}}

    def test_json(self, tmp_path):
        p = tmp_path / "x.json"
        p.write_text(json.dumps({"top_p": 0.9}))
        data, err = _load_params_file(str(p))
        assert err is None
        assert data == {"top_p": 0.9}

    def test_missing(self, tmp_path):
        data, err = _load_params_file(str(tmp_path / "missing.yaml"))
        assert data is None
        assert err and "Cannot read" in err

    def test_not_a_dict(self, tmp_path):
        p = tmp_path / "list.json"
        p.write_text("[1, 2, 3]")
        data, err = _load_params_file(str(p))
        assert data is None
        assert "top-level dict" in err


# ---------------------------------------------------------------------------
# Generalized prompt JSON: --set and --unset on prompts set
# ---------------------------------------------------------------------------


class TestPromptOptionsGeneralized:
    def test_set_dotted_path(self):
        base = {"prompt": "", "max_tokens": 256, "temperature": 0.7, "role": "user"}
        result, err = parse_prompt_options(
            ["--prompt", "hi", "--set", "reasoning.effort=high"],
            base,
            require_prompt=False,
        )
        assert err is None
        assert result["prompt"] == "hi"
        assert result["reasoning"] == {"effort": "high"}

    def test_set_overwrites_top_level(self):
        base = {"prompt": "", "max_tokens": 256, "temperature": 0.7, "role": "user"}
        result, err = parse_prompt_options(
            ["--set", "temperature=1.5", "hello"], base, require_prompt=True
        )
        assert err is None
        assert result["temperature"] == 1.5

    def test_unset_removes_field(self):
        base = {"prompt": "hi", "max_tokens": 256, "temperature": 0.7, "role": "user"}
        result, err = parse_prompt_options(
            ["--unset", "max_tokens"], base, require_prompt=True
        )
        assert err is None
        assert "max_tokens" not in result


# ---------------------------------------------------------------------------
# End-to-end: handle_respond drives the new flag pipeline
# ---------------------------------------------------------------------------


class TestHandleRespondEndToEnd:
    """Verify handle_respond translates schema flags into composed body fields."""

    @pytest.fixture
    def setup_provider(self, tmp_path, monkeypatch, api_def):
        """Set up a provider that references our test API def, plus a stub HTTP layer."""
        from lexi_cli import cli

        # Patch APP_DIR-related paths
        app_dir = tmp_path / "lexi"
        app_dir.mkdir()
        apis_dir = app_dir / "apis"
        apis_dir.mkdir()
        prov_dir = app_dir / "providers"
        prov_dir.mkdir()

        # Write the API def
        (apis_dir / "responses.yaml").write_text(_yaml_dump(api_def))

        # Write a providers-config.yaml referencing it
        providers_cfg = {
            "providers": {
                "test": {
                    "url": "https://api.example.com",
                    "default_model": "test-model",
                    "api": "responses",
                }
            }
        }
        (prov_dir / "providers-config.yaml").write_text(_yaml_dump(providers_cfg))

        # Write an api-key file
        provider_dir = prov_dir / "test"
        provider_dir.mkdir()
        (provider_dir / "api-key").write_text("sk-test")
        (provider_dir / "api-key").chmod(0o600)

        # Patch globals
        monkeypatch.setattr(cli, "APP_DIR", app_dir)
        monkeypatch.setattr(cli, "APIS_DIR", apis_dir)
        monkeypatch.setattr(cli, "PROVIDERS_ROOT", prov_dir)
        monkeypatch.setattr(cli, "PROVIDERS_CONFIG", prov_dir / "providers-config.yaml")
        monkeypatch.setattr(cli, "PROMPTS", {"$$": {"prompt": "hello", "role": "user"}})

        captured: dict = {}

        # Realistic Responses-API body: a web_search_call comes first,
        # then a message item with output_text content carrying url_citation
        # annotations. This is what surfaces the bug Step 1 fixed and
        # exercises the citations path Step 2 added.
        _fake_body = {
            "id": "resp_test",
            "output": [
                {"type": "web_search_call", "id": "ws_1", "status": "completed"},
                {
                    "type": "message",
                    "role": "assistant",
                    "content": [
                        {
                            "type": "output_text",
                            "text": "Recent NVDA news…",
                            "annotations": [
                                {
                                    "type": "url_citation",
                                    "url": "https://example.com/a",
                                    "title": "Article A",
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
            ],
            "usage": {},
            "status": "completed",
        }

        class FakeJsonResponse:
            def read(self):
                return json.dumps(_fake_body).encode()

            def __enter__(self):
                return self

            def __exit__(self, *a):
                return False

        class FakeSSEResponse:
            """Iterable that yields SSE lines (bytes) for the Responses API."""

            def __init__(self, chunks):
                lines = []
                for chunk in chunks:
                    lines.append(b"event: response.output_text.delta\n")
                    lines.append(b"data: " + json.dumps({"delta": chunk}).encode() + b"\n")
                    lines.append(b"\n")
                lines.append(b"event: response.completed\n")
                lines.append(b"data: " + json.dumps({"id": "resp_1", "status": "completed"}).encode() + b"\n")
                lines.append(b"\n")
                self._lines = lines

            def __iter__(self):
                return iter(self._lines)

            def __enter__(self):
                return self

            def __exit__(self, *a):
                return False

        def fake_urlopen(req, timeout=30):
            captured["url"] = req.full_url
            captured["body"] = json.loads(req.data.decode())
            captured["headers"] = dict(req.header_items())
            if captured["body"].get("stream"):
                return FakeSSEResponse(["Hel", "lo"])
            return FakeJsonResponse()

        monkeypatch.setattr("urllib.request.urlopen", fake_urlopen)
        return cli, captured

    def test_temperature_via_flag(self, setup_provider):
        cli, captured = setup_provider
        code = cli.handle_respond("test --temperature 0.7 -r")
        assert code == 0
        assert captured["body"]["temperature"] == 0.7

    def test_max_tokens_routes_to_max_output_tokens(self, setup_provider):
        cli, captured = setup_provider
        code = cli.handle_respond("test --max_tokens 50 -r")
        assert code == 0
        # Schema property is max_output_tokens; the --max-tokens flag maps to it
        assert captured["body"]["max_output_tokens"] == 50

    def test_stream_switch_sets_body_field(self, setup_provider, capsys):
        cli, captured = setup_provider
        # No --raw: deltas should print to stdout incrementally.
        code = cli.handle_respond("test --stream")
        assert code == 0
        assert captured["body"]["stream"] is True
        out = capsys.readouterr().out
        # Concatenated deltas with trailing newline.
        assert out == "Hello\n"

    def test_stream_with_raw_suppresses_deltas(self, setup_provider, capsys):
        cli, captured = setup_provider
        code = cli.handle_respond("test --stream -r")
        assert code == 0
        out = capsys.readouterr().out
        # No delta text in stdout; instead a JSON dump of the final response.
        assert "Hello" not in out
        assert '"status": "completed"' in out

    def test_stream_with_tool_activity_events(self, setup_provider, capsys, monkeypatch):
        cli, _ = setup_provider

        # Replace urlopen with one that emits a tool-rich SSE event sequence.
        sse_lines = [
            b"event: response.web_search_call.searching\n",
            b"data: {}\n",
            b"\n",
            b"event: response.web_search_call.completed\n",
            b"data: {}\n",
            b"\n",
            b"event: response.output_text.delta\n",
            b"data: " + json.dumps({"delta": "Result"}).encode() + b"\n",
            b"\n",
            b"event: response.completed\n",
            b"data: " + json.dumps({"id": "r1", "status": "completed"}).encode() + b"\n",
            b"\n",
        ]

        class FakeSSE:
            def __iter__(self_inner):
                return iter(sse_lines)
            def __enter__(self_inner):
                return self_inner
            def __exit__(self_inner, *a):
                return False

        def fake_urlopen(req, timeout=30):
            return FakeSSE()

        monkeypatch.setattr("urllib.request.urlopen", fake_urlopen)

        code = cli.handle_respond("test --stream")
        assert code == 0
        captured = capsys.readouterr()
        # Answer text on stdout.
        assert captured.out == "Result\n"
        # Tool indicators on stderr.
        assert "🔍 searching" in captured.err
        assert "🔍 done" in captured.err

    def test_set_dotted_path(self, setup_provider):
        cli, captured = setup_provider
        code = cli.handle_respond("test --set reasoning.effort=high -r")
        assert code == 0
        assert captured["body"]["reasoning"] == {"effort": "high"}

    def test_tool_preset(self, setup_provider):
        cli, captured = setup_provider
        code = cli.handle_respond("test --tool web_search -r")
        assert code == 0
        assert captured["body"]["tools"] == [{"type": "web_search"}]

    def test_unknown_flag_errors(self, setup_provider, capsys):
        cli, _ = setup_provider
        assert cli.handle_respond("test --bogus 1") == 1
        # print_error writes to stdout (with ANSI red); strip ANSI when checking.
        captured = capsys.readouterr()
        combined = captured.out + captured.err
        assert "Unknown option" in combined

    def test_validation_error_returns_one(self, setup_provider, capsys):
        cli, _ = setup_provider
        assert cli.handle_respond("test --temperature 99") == 1
        captured = capsys.readouterr()
        combined = (captured.out + captured.err).lower()
        assert "validation" in combined or "must be" in combined

    def test_text_extracted_through_web_search_call(self, setup_provider, capsys):
        # Without --cite: just the text, no Sources section.
        # The fixture's response has output[0]=web_search_call, output[1]=message.
        cli, _ = setup_provider
        code = cli.handle_respond("test")
        assert code == 0
        out = capsys.readouterr().out
        assert "Recent NVDA news" in out
        assert "Sources:" not in out

    def test_cite_flag_prints_sources(self, setup_provider, capsys):
        cli, _ = setup_provider
        code = cli.handle_respond("test --cite")
        assert code == 0
        out = capsys.readouterr().out
        assert "Recent NVDA news" in out
        assert "Sources:" in out
        assert "[1] https://example.com/a — Article A" in out
        assert "[2] https://example.com/b — Article B" in out

    def test_verbose_includes_activity_footer(self, setup_provider, capsys):
        cli, _ = setup_provider
        # Fixture's response has 1 web_search_call + 1 message.
        # --verbose should print metadata and an activity line on stderr.
        code = cli.handle_respond("test --verbose")
        assert code == 0
        captured = capsys.readouterr()
        # Activity summary lands on stderr (dim styling).
        assert "1 web search" in captured.err

    def test_verbose_skips_activity_when_no_tool_items(self, setup_provider, capsys, monkeypatch):
        # Stub a response with NO web_search_call — only a message item.
        cli, _ = setup_provider

        plain_body = {
            "id": "resp_x",
            "output": [
                {
                    "type": "message",
                    "content": [{"type": "output_text", "text": "plain answer"}],
                }
            ],
            "usage": {},
        }

        class FakeJsonResponse:
            def read(self_inner):
                return json.dumps(plain_body).encode()
            def __enter__(self_inner):
                return self_inner
            def __exit__(self_inner, *a):
                return False

        def fake_urlopen(req, timeout=30):
            return FakeJsonResponse()

        monkeypatch.setattr("urllib.request.urlopen", fake_urlopen)

        code = cli.handle_respond("test --verbose")
        assert code == 0
        captured = capsys.readouterr()
        # No tool items → no activity line printed.
        assert "web search" not in captured.err
        assert "reasoning" not in captured.err

    def test_cite_without_annotations_block_is_silent(self, setup_provider, capsys, monkeypatch):
        # Strip the annotations block from the on-disk YAML and reload.
        cli, _ = setup_provider
        # Reach into the apis dir we patched and strip annotations from the file.
        api_path = cli.APIS_DIR / "responses.yaml"
        text = api_path.read_text()
        # Crude but sufficient: blank out the annotations key so YAML parser ignores it.
        # We just rewrite the file with the annotations key removed by reading via yaml.
        import yaml as _yaml

        data = _yaml.safe_load(text)
        data["response"].pop("annotations", None)
        api_path.write_text(_yaml.safe_dump(data, sort_keys=False))

        code = cli.handle_respond("test --cite")
        assert code == 0
        out = capsys.readouterr().out
        assert "Recent NVDA news" in out
        # No Sources section printed when the API doesn't declare annotations.
        assert "Sources:" not in out


# ---------------------------------------------------------------------------
# SSE parser + streaming decoder
# ---------------------------------------------------------------------------


class TestSSEReader:
    def _lines(self, *raw_blocks):
        out = []
        for block in raw_blocks:
            out.extend((line + "\n").encode() for line in block.split("\n"))
        return out

    def test_named_event(self):
        events = list(_read_sse_events(self._lines("event: foo\ndata: {\"x\": 1}\n")))
        assert events == [("foo", '{"x": 1}')]

    def test_unnamed_event(self):
        events = list(_read_sse_events(self._lines("data: {\"y\": 2}\n")))
        assert events == [(None, '{"y": 2}')]

    def test_multiple_events(self):
        events = list(_read_sse_events(self._lines(
            "event: a\ndata: 1\n\nevent: b\ndata: 2\n"
        )))
        assert events == [("a", "1"), ("b", "2")]

    def test_multiline_data(self):
        events = list(_read_sse_events(self._lines(
            "event: x\ndata: line1\ndata: line2\n"
        )))
        assert events == [("x", "line1\nline2")]

    def test_comment_ignored(self):
        events = list(_read_sse_events(self._lines(
            ": this is a heartbeat\nevent: x\ndata: ok\n"
        )))
        assert events == [("x", "ok")]


class TestStreamSseResponse:
    def _resp(self, lines):
        return [(line + "\n").encode() for line in lines]

    def test_named_event_deltas(self, capsys):
        cfg = {
            "delta_event": "response.output_text.delta",
            "delta_path": "delta",
            "done_event": "response.completed",
            "final_event": "response.completed",
        }
        resp = self._resp([
            "event: response.output_text.delta",
            "data: {\"delta\": \"Hel\"}",
            "",
            "event: response.output_text.delta",
            "data: {\"delta\": \"lo\"}",
            "",
            "event: response.completed",
            "data: {\"status\": \"completed\"}",
            "",
        ])
        final = _stream_sse_response(resp, cfg)
        out = capsys.readouterr().out
        assert out == "Hello\n"
        assert final == {"status": "completed"}

    def test_done_marker(self, capsys):
        # Chat-Completions-style: unnamed events, terminated by data: [DONE]
        cfg = {
            "delta_event": None,
            "delta_path": "choices[0].delta.content",
            "done_marker": "[DONE]",
        }
        resp = self._resp([
            "data: {\"choices\": [{\"delta\": {\"content\": \"Hel\"}}]}",
            "",
            "data: {\"choices\": [{\"delta\": {\"content\": \"lo\"}}]}",
            "",
            "data: [DONE]",
            "",
        ])
        final = _stream_sse_response(resp, cfg)
        out = capsys.readouterr().out
        assert out == "Hello\n"
        assert final is None  # done_marker doesn't carry a final response

    def test_activity_events_print_to_stderr(self, capsys):
        cfg = {
            "delta_event": "response.output_text.delta",
            "delta_path": "delta",
            "done_event": "response.completed",
            "final_event": "response.completed",
            "activity_events": {
                "response.web_search_call.searching": "🔍 searching",
                "response.web_search_call.completed": "🔍 done",
            },
        }
        resp = self._resp([
            "event: response.web_search_call.searching",
            "data: {}",
            "",
            "event: response.web_search_call.completed",
            "data: {}",
            "",
            "event: response.output_text.delta",
            "data: {\"delta\": \"Hello\"}",
            "",
            "event: response.completed",
            "data: {\"status\": \"completed\"}",
            "",
        ])
        final = _stream_sse_response(resp, cfg)
        captured = capsys.readouterr()
        # Text on stdout, indicators on stderr.
        assert captured.out == "Hello\n"
        assert "🔍 searching" in captured.err
        assert "🔍 done" in captured.err
        assert final == {"status": "completed"}

    def test_unknown_activity_event_silent(self, capsys):
        cfg = {
            "delta_event": "delta",
            "delta_path": "v",
            "done_event": "done",
            "activity_events": {"known_event": "✓"},
        }
        resp = self._resp([
            "event: unknown_event",
            "data: {}",
            "",
            "event: delta",
            "data: {\"v\": \"x\"}",
            "",
            "event: done",
            "data: {}",
            "",
        ])
        _stream_sse_response(resp, cfg)
        captured = capsys.readouterr()
        assert captured.err == ""  # nothing matched

    def test_empty_message_skipped(self, capsys):
        cfg = {
            "delta_event": "delta",
            "delta_path": "v",
            "done_event": "done",
            "activity_events": {"ping": ""},  # empty string → no print
        }
        resp = self._resp([
            "event: ping",
            "data: {}",
            "",
            "event: done",
            "data: {}",
            "",
        ])
        _stream_sse_response(resp, cfg)
        captured = capsys.readouterr()
        assert captured.err == ""

    def test_suppress_text_for_raw_mode(self, capsys):
        cfg = {
            "delta_event": "delta",
            "delta_path": "v",
            "done_event": "done",
            "final_event": "done",
        }
        resp = self._resp([
            "event: delta",
            "data: {\"v\": \"X\"}",
            "",
            "event: done",
            "data: {\"id\": \"1\"}",
            "",
        ])
        final = _stream_sse_response(resp, cfg, suppress_text=True)
        out = capsys.readouterr().out
        assert out == ""  # suppressed
        assert final == {"id": "1"}


def _yaml_dump(obj):
    """Tiny YAML dumper using PyYAML if available, else json fallback."""
    try:
        import yaml  # type: ignore

        return yaml.safe_dump(obj, sort_keys=False)
    except ImportError:
        return json.dumps(obj)
