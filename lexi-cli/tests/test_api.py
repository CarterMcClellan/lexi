#!/usr/bin/env python3
"""Tests for the schema-driven API abstraction layer.

The legacy ``parameters``/``param_mapping``/``respond_kind`` paths were removed in
the Phase 2 cut-over. These tests cover the new ``request.body_schema`` shape and
the integration between cli.py and the api_compose engine.
"""

from __future__ import annotations

import json
from unittest.mock import patch

import pytest

from lexi_cli.cli import (
    deep_merge,
    delete_api_definition,
    extract_response_metadata,
    load_all_api_definitions,
    load_api_definition,
    main,
    resolve_effective_config,
    save_api_definition,
    seed_default_apis,
    validate_api_definition,
)

# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------


@pytest.fixture
def apis_dir(tmp_path):
    """Patch APIS_DIR + DEFAULT_APIS_ROOT to point at temporary directories."""
    d = tmp_path / "apis"
    d.mkdir()
    empty_default = tmp_path / "no_defaults"
    empty_default.mkdir()
    with patch("lexi_cli.cli.APIS_DIR", d), patch("lexi_cli.cli.DEFAULT_APIS_ROOT", empty_default):
        yield d


@pytest.fixture
def sample_api():
    """A minimal but valid schema-driven API definition (chat-completions style)."""
    return {
        "name": "test-api",
        "description": "Test API",
        "auth": {"header": "Authorization", "value_prefix": "Bearer "},
        "request": {
            "method": "POST",
            "path": "/chat/completions",
            "body_schema": {
                "type": "object",
                "required": ["model", "messages"],
                "properties": {
                    "model": {"type": "string", "x-bind": {"from": "model"}},
                    "messages": {
                        "type": "array",
                        "x-render": {
                            "template": [
                                {"role": "{{role}}", "content": "{{prompt}}"}
                            ]
                        },
                    },
                    "temperature": {
                        "type": "number",
                        "minimum": 0,
                        "maximum": 2,
                        "x-cli": {"flag": "--temperature", "alias": "-t"},
                    },
                    "max_tokens": {
                        "type": "integer",
                        "minimum": 1,
                        "x-cli": {"flag": "--max-tokens", "alias": "-m"},
                    },
                },
            },
        },
        "response": {
            "text_extract": {"text_path": "choices[0].message.content"},
            "metadata": {
                "model": "model",
                "input_tokens": "usage.prompt_tokens",
                "output_tokens": "usage.completion_tokens",
                "total_tokens": "usage.total_tokens",
                "stop_reason": "choices[0].finish_reason",
            },
        },
    }


# ---------------------------------------------------------------------------
# deep_merge — utility used by resolve_effective_config
# ---------------------------------------------------------------------------


class TestDeepMerge:
    def test_flat_merge(self):
        assert deep_merge({"a": 1}, {"b": 2}) == {"a": 1, "b": 2}

    def test_override(self):
        assert deep_merge({"a": 1}, {"a": 2}) == {"a": 2}

    def test_nested_merge(self):
        base = {"auth": {"header": "Authorization", "prefix": "Bearer "}}
        override = {"auth": {"prefix": "Token "}}
        assert deep_merge(base, override) == {
            "auth": {"header": "Authorization", "prefix": "Token "}
        }

    def test_list_replaced(self):
        assert deep_merge({"items": [1, 2, 3]}, {"items": [4, 5]}) == {"items": [4, 5]}


# ---------------------------------------------------------------------------
# CRUD on API definitions
# ---------------------------------------------------------------------------


class TestAPICrud:
    def test_load_nonexistent(self, apis_dir):
        assert load_api_definition("nonexistent") is None

    def test_save_and_load(self, apis_dir, sample_api):
        assert save_api_definition("test-api", sample_api)
        loaded = load_api_definition("test-api")
        assert loaded is not None
        assert loaded["name"] == "test-api"
        assert loaded["request"]["body_schema"]["properties"]["model"]["x-bind"]["from"] == "model"

    def test_load_all(self, apis_dir, sample_api):
        save_api_definition("api-a", {**sample_api, "name": "api-a"})
        save_api_definition("api-b", {**sample_api, "name": "api-b"})
        all_apis = load_all_api_definitions()
        assert {"api-a", "api-b"}.issubset(all_apis.keys())

    def test_delete(self, apis_dir, sample_api):
        save_api_definition("to-delete", sample_api)
        assert delete_api_definition("to-delete")
        assert load_api_definition("to-delete") is None

    def test_delete_nonexistent(self, apis_dir):
        assert not delete_api_definition("nonexistent")


# ---------------------------------------------------------------------------
# validate_api_definition (schema-aware)
# ---------------------------------------------------------------------------


class TestValidateAPIDefinition:
    def test_valid(self, sample_api):
        assert validate_api_definition(sample_api) == []

    def test_missing_name(self, sample_api):
        del sample_api["name"]
        assert any("name" in e for e in validate_api_definition(sample_api))

    def test_missing_request(self, sample_api):
        del sample_api["request"]
        assert any("request" in e for e in validate_api_definition(sample_api))

    def test_missing_path(self, sample_api):
        del sample_api["request"]["path"]
        assert any("path" in e for e in validate_api_definition(sample_api))

    def test_missing_body_schema(self, sample_api):
        del sample_api["request"]["body_schema"]
        errors = validate_api_definition(sample_api)
        assert any("body_schema" in e for e in errors)

    def test_rejects_legacy_body(self, sample_api):
        sample_api["request"]["body"] = {"model": "{{model}}"}
        errors = validate_api_definition(sample_api)
        assert any("legacy" in e.lower() for e in errors)

    def test_rejects_legacy_parameters(self, sample_api):
        sample_api["request"]["parameters"] = {"temperature": {"body_field": "temperature"}}
        errors = validate_api_definition(sample_api)
        assert any("legacy" in e.lower() for e in errors)

    def test_invalid_x_include(self, sample_api):
        sample_api["request"]["body_schema"]["properties"]["temperature"]["x-include"] = "wrong"
        errors = validate_api_definition(sample_api)
        assert any("x-include" in e for e in errors)

    def test_missing_response(self, sample_api):
        del sample_api["response"]
        assert any("response" in e for e in validate_api_definition(sample_api))

    def test_missing_text_extract(self, sample_api):
        del sample_api["response"]["text_extract"]
        assert any("text_extract" in e for e in validate_api_definition(sample_api))

    def test_rejects_legacy_text_path(self, sample_api):
        sample_api["response"]["text_path"] = "x"
        assert any("legacy" in e.lower() for e in validate_api_definition(sample_api))

    def test_text_extract_requires_one_of_path_or_items(self, sample_api):
        sample_api["response"]["text_extract"] = {}
        assert any("text_path or items_path" in e for e in validate_api_definition(sample_api))

    def test_text_extract_rejects_both_modes(self, sample_api):
        sample_api["response"]["text_extract"] = {"text_path": "a", "items_path": "b"}
        assert any("not both" in e for e in validate_api_definition(sample_api))

    def test_tool_presets_must_be_dict(self, sample_api):
        sample_api["tool_presets"] = "nope"
        assert any("tool_presets" in e for e in validate_api_definition(sample_api))


# ---------------------------------------------------------------------------
# Seeding
# ---------------------------------------------------------------------------


class TestSeedDefaultAPIs:
    def test_seed_copies_files(self, tmp_path):
        src = tmp_path / "default_apis"
        src.mkdir()
        (src / "test.yaml").write_text("name: test\n")
        dest = tmp_path / "apis"
        dest.mkdir()
        with patch("lexi_cli.cli.DEFAULT_APIS_ROOT", src), patch("lexi_cli.cli.APIS_DIR", dest):
            seed_default_apis()
        assert (dest / "test.yaml").exists()

    def test_seed_does_not_overwrite(self, tmp_path):
        src = tmp_path / "default_apis"
        src.mkdir()
        (src / "test.yaml").write_text("name: new\n")
        dest = tmp_path / "apis"
        dest.mkdir()
        (dest / "test.yaml").write_text("name: existing\n")
        with patch("lexi_cli.cli.DEFAULT_APIS_ROOT", src), patch("lexi_cli.cli.APIS_DIR", dest):
            seed_default_apis()
        assert "existing" in (dest / "test.yaml").read_text()


# ---------------------------------------------------------------------------
# resolve_effective_config (no more legacy fallback)
# ---------------------------------------------------------------------------


class TestResolveEffectiveConfig:
    def test_api_reference(self, apis_dir, sample_api):
        save_api_definition("test-api", sample_api)
        provider_cfg = {"url": "https://example.com", "api": "test-api"}
        result = resolve_effective_config("test", provider_cfg)
        assert result["request"]["path"] == "/chat/completions"
        assert result["response"]["text_extract"]["text_path"] == "choices[0].message.content"
        assert "body_schema" in result["request"]

    def test_api_with_inline_auth_override(self, apis_dir, sample_api):
        save_api_definition("test-api", sample_api)
        provider_cfg = {
            "url": "https://example.com",
            "api": "test-api",
            "auth": {"header": "x-custom-key", "value_prefix": ""},
        }
        result = resolve_effective_config("test", provider_cfg)
        assert result["auth"]["header"] == "x-custom-key"
        assert result["auth"]["value_prefix"] == ""

    def test_inline_request_no_api(self):
        provider_cfg = {
            "url": "https://example.com",
            "request": {
                "path": "/inline",
                "body_schema": {
                    "type": "object",
                    "properties": {"model": {"type": "string", "x-bind": {"from": "model"}}},
                },
            },
            "response": {"text_extract": {"text_path": "text"}},
        }
        result = resolve_effective_config("test", provider_cfg)
        assert result["request"]["path"] == "/inline"

    def test_missing_api_def_returns_provider_cfg(self, apis_dir):
        # No legacy fallback — caller will fail later when no body_schema is found.
        provider_cfg = {"url": "https://example.com", "api": "nonexistent"}
        result = resolve_effective_config("test", provider_cfg)
        assert result == provider_cfg

    def test_tool_presets_merged(self, apis_dir, sample_api):
        sample_api["tool_presets"] = {"web_search": {"tools": [{"type": "web_search"}]}}
        save_api_definition("test-api", sample_api)
        provider_cfg = {
            "url": "https://example.com",
            "api": "test-api",
            "tool_presets": {"my_preset": {"foo": "bar"}},
        }
        result = resolve_effective_config("test", provider_cfg)
        assert "web_search" in result["tool_presets"]
        assert result["tool_presets"]["my_preset"] == {"foo": "bar"}


# ---------------------------------------------------------------------------
# extract_response_metadata (unchanged behavior)
# ---------------------------------------------------------------------------


class TestExtractResponseMetadata:
    def test_chat_completions_response(self, sample_api):
        response = {
            "model": "gpt-4o",
            "choices": [{"message": {"content": "Hello"}, "finish_reason": "stop"}],
            "usage": {"prompt_tokens": 10, "completion_tokens": 20, "total_tokens": 30},
        }
        meta = extract_response_metadata(response, sample_api)
        assert meta["model"] == "gpt-4o"
        assert meta["input_tokens"] == 10
        assert meta["output_tokens"] == 20
        assert meta["total_tokens"] == 30
        assert meta["stop_reason"] == "stop"

    def test_partial_metadata(self, sample_api):
        meta = extract_response_metadata({"model": "test"}, sample_api)
        assert meta["model"] == "test"
        assert "input_tokens" not in meta

    def test_no_metadata_config(self):
        assert extract_response_metadata({"model": "test"}, {"response": {}}) == {}


# ---------------------------------------------------------------------------
# api command (CLI dispatch via main)
# ---------------------------------------------------------------------------


class TestAPICommand:
    def test_api_list_empty(self, apis_dir, capsys):
        assert main(["api", "list"]) == 0
        assert "No API definitions" in capsys.readouterr().out

    def test_api_list_with_entries(self, apis_dir, sample_api, capsys):
        save_api_definition("my-api", sample_api)
        assert main(["api", "list"]) == 0
        assert "my-api" in capsys.readouterr().out

    def test_api_list_raw(self, apis_dir, sample_api, capsys):
        save_api_definition("my-api", sample_api)
        assert main(["api", "list", "--raw"]) == 0
        data = json.loads(capsys.readouterr().out)
        assert "my-api" in data

    def test_api_show(self, apis_dir, sample_api, capsys):
        save_api_definition("my-api", sample_api)
        assert main(["api", "show", "my-api"]) == 0
        assert "my-api" in capsys.readouterr().out

    def test_api_show_raw(self, apis_dir, sample_api, capsys):
        save_api_definition("my-api", sample_api)
        assert main(["api", "show", "my-api", "--raw"]) == 0
        data = json.loads(capsys.readouterr().out)
        assert data["name"] == "test-api"

    def test_api_show_not_found(self, apis_dir):
        assert main(["api", "show", "nonexistent"]) == 1

    def test_api_create_default_template(self, apis_dir):
        assert main(["api", "create", "new-api"]) == 0
        loaded = load_api_definition("new-api")
        assert loaded is not None
        assert loaded["name"] == "new-api"
        # Default template uses the new schema-driven shape
        assert "body_schema" in loaded["request"]
        assert "model" in loaded["request"]["body_schema"]["properties"]

    def test_api_create_default_template_validates(self, apis_dir):
        main(["api", "create", "new-api"])
        loaded = load_api_definition("new-api")
        assert validate_api_definition(loaded) == []

    def test_api_create_from_existing(self, apis_dir, sample_api):
        save_api_definition("source", sample_api)
        assert main(["api", "create", "cloned", "--from", "source"]) == 0
        loaded = load_api_definition("cloned")
        assert loaded["name"] == "cloned"
        assert loaded["request"]["path"] == "/chat/completions"
        assert "body_schema" in loaded["request"]

    def test_api_create_duplicate(self, apis_dir, sample_api):
        save_api_definition("existing", sample_api)
        assert main(["api", "create", "existing"]) == 1

    def test_api_create_with_path(self, apis_dir):
        assert main(["api", "create", "custom", "--path", "/v2/chat", "--text-path", "result.text"]) == 0
        loaded = load_api_definition("custom")
        assert loaded["request"]["path"] == "/v2/chat"
        assert loaded["response"]["text_extract"]["text_path"] == "result.text"

    def test_api_edit_path(self, apis_dir, sample_api):
        save_api_definition("edit-me", sample_api)
        assert main(["api", "edit", "edit-me", "--path", "/v2/completions"]) == 0
        assert load_api_definition("edit-me")["request"]["path"] == "/v2/completions"

    def test_api_edit_text_path(self, apis_dir, sample_api):
        save_api_definition("edit-me", sample_api)
        assert main(["api", "edit", "edit-me", "--text-path", "result.text"]) == 0
        assert load_api_definition("edit-me")["response"]["text_extract"]["text_path"] == "result.text"

    def test_api_edit_set_meta(self, apis_dir, sample_api):
        save_api_definition("edit-me", sample_api)
        assert main(["api", "edit", "edit-me", "--set-meta", "latency:timing.total_ms"]) == 0
        assert load_api_definition("edit-me")["response"]["metadata"]["latency"] == "timing.total_ms"

    def test_api_edit_not_found(self, apis_dir):
        assert main(["api", "edit", "nonexistent", "--path", "/v2"]) == 1

    def test_api_rm(self, apis_dir, sample_api):
        save_api_definition("to-delete", sample_api)
        assert main(["api", "rm", "to-delete"]) == 0
        assert load_api_definition("to-delete") is None

    def test_api_rm_not_found(self, apis_dir):
        assert main(["api", "rm", "nonexistent"]) == 1

    def test_api_validate_valid(self, apis_dir, sample_api, capsys):
        save_api_definition("valid-api", sample_api)
        assert main(["api", "validate", "valid-api"]) == 0
        assert "valid" in capsys.readouterr().out.lower()

    def test_api_validate_invalid(self, apis_dir):
        save_api_definition("bad-api", {"name": "bad-api"})
        assert main(["api", "validate", "bad-api"]) == 1

    def test_api_validate_not_found(self, apis_dir):
        assert main(["api", "validate", "nonexistent"]) == 1

    def test_api_no_subcommand(self):
        assert main(["api"]) == 1

    def test_api_unknown_subcommand(self):
        assert main(["api", "unknown"]) == 1

    def test_api_preview_named(self, apis_dir, sample_api, capsys):
        save_api_definition("preview-target", sample_api)
        assert main(["api", "preview", "preview-target"]) == 0
        out = capsys.readouterr().out
        data = json.loads(out)
        assert data["valid"] is True
        assert data["name"] == "test-api"
        assert isinstance(data["flags"], list)
        assert "body" in data
        assert data["body"]["model"] == "sample-model"

    def test_api_preview_with_sample_inputs(self, apis_dir, sample_api, capsys):
        save_api_definition("preview-target", sample_api)
        assert main([
            "api", "preview", "preview-target",
            "--prompt", "hi there",
            "--model", "my-model",
            "--role", "system",
        ]) == 0
        data = json.loads(capsys.readouterr().out)
        assert data["body"]["model"] == "my-model"
        assert data["body"]["messages"] == [{"role": "system", "content": "hi there"}]

    def test_api_preview_from_file(self, apis_dir, sample_api, tmp_path, capsys):
        try:
            import yaml as _yaml
        except ImportError:
            pytest.skip("PyYAML not installed")
        p = tmp_path / "x.yaml"
        p.write_text(_yaml.safe_dump(sample_api))
        assert main(["api", "preview", "--file", str(p)]) == 0
        data = json.loads(capsys.readouterr().out)
        assert data["valid"] is True

    def test_api_preview_file_with_invalid_schema(self, apis_dir, tmp_path, capsys):
        p = tmp_path / "bad.yaml"
        p.write_text("name: bad\n")  # missing request/response
        assert main(["api", "preview", "--file", str(p)]) == 0
        data = json.loads(capsys.readouterr().out)
        assert data["valid"] is False
        assert data["errors"]
        assert "body" not in data

    def test_api_preview_not_found(self, apis_dir):
        assert main(["api", "preview", "ghost"]) == 1


# ---------------------------------------------------------------------------
# End-to-end integration: respond_with_provider builds the right body
# ---------------------------------------------------------------------------


class TestRespondWithProvider:
    """Verify that respond_with_provider drives api_compose correctly."""

    def _provider_cfg(self, sample_api):
        # Inline the API definition under the provider so we don't need disk I/O.
        return {
            "url": "https://api.example.com",
            "default_model": "test-model",
            **{k: sample_api[k] for k in ("auth", "request", "response")},
        }

    def test_compose_body_for_chat_completions(self, sample_api, monkeypatch):
        from lexi_cli import cli

        captured: dict = {}

        def fake_load_api_key(name, cfg):
            return "sk-test"

        class FakeResponse:
            def read(self):
                return b'{"choices":[{"message":{"content":"hi"}}]}'

            def __enter__(self):
                return self

            def __exit__(self, *a):
                return False

        def fake_urlopen(req, timeout=30):
            captured["url"] = req.full_url
            captured["body"] = json.loads(req.data.decode())
            captured["headers"] = dict(req.header_items())
            return FakeResponse()

        monkeypatch.setattr(cli, "load_api_key", fake_load_api_key)
        monkeypatch.setattr("urllib.request.urlopen", fake_urlopen)

        provider_cfg = self._provider_cfg(sample_api)
        prompt_payload = {"prompt": "hello world", "role": "user"}

        code, resp_json, _streamed = cli.respond_with_provider(
            "test",
            provider_cfg,
            prompt_payload,
            overrides={"temperature": 0.5},
        )

        assert code == 0
        assert captured["url"] == "https://api.example.com/chat/completions"
        assert captured["body"]["model"] == "test-model"
        assert captured["body"]["messages"] == [{"role": "user", "content": "hello world"}]
        assert captured["body"]["temperature"] == 0.5
        assert "Authorization" in captured["headers"]
        assert resp_json["choices"][0]["message"]["content"] == "hi"

    def test_validation_error_returns_nonzero(self, sample_api, monkeypatch):
        from lexi_cli import cli

        monkeypatch.setattr(cli, "load_api_key", lambda *a: "sk-test")
        monkeypatch.setattr(
            "urllib.request.urlopen", lambda *a, **k: pytest.fail("should not reach HTTP")
        )

        provider_cfg = self._provider_cfg(sample_api)
        prompt_payload = {"prompt": "hi", "role": "user"}
        # Out-of-range temperature triggers validation failure
        code, resp, _streamed = cli.respond_with_provider(
            "test", provider_cfg, prompt_payload, overrides={"temperature": 99}
        )
        assert code == 1
        assert resp is None

    def test_tool_preset_injection(self, sample_api, monkeypatch):
        from lexi_cli import cli

        sample_api["tool_presets"] = {
            "web_search": {"tools": [{"type": "web_search"}]}
        }
        # Make sure our schema permits a tools array (it does by default since
        # schemas are open unless additionalProperties:false; we just need it
        # to not reject).
        sample_api["request"]["body_schema"]["properties"]["tools"] = {
            "type": "array",
            "items": {"type": "object"},
        }

        captured: dict = {}

        class FakeResponse:
            def read(self):
                return b"{}"

            def __enter__(self):
                return self

            def __exit__(self, *a):
                return False

        def fake_urlopen(req, timeout=30):
            captured["body"] = json.loads(req.data.decode())
            return FakeResponse()

        monkeypatch.setattr(cli, "load_api_key", lambda *a: "sk-test")
        monkeypatch.setattr("urllib.request.urlopen", fake_urlopen)

        provider_cfg = self._provider_cfg(sample_api)
        provider_cfg["tool_presets"] = sample_api["tool_presets"]

        code, _, _streamed = cli.respond_with_provider(
            "test",
            provider_cfg,
            {"prompt": "hi", "role": "user"},
            tool_presets_selected=["web_search"],
        )
        assert code == 0
        assert captured["body"]["tools"] == [{"type": "web_search"}]
