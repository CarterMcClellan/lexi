"""The admin namespaces — lexi.providers / models / prompts / config / apis —
over a temp ``$LEXI_HOME`` seeded with the shipped defaults."""

import pytest

import lexi
from conftest import Mock, ok


def test_providers_lifecycle(home):
    names = [p["name"] for p in lexi.providers.list()]
    assert {"openai", "anthropic", "nvidia"} <= set(names)
    row = next(p for p in lexi.providers.list() if p["name"] == "openai")
    assert set(row) == {
        "name", "url", "api", "default_model", "hosted_models", "hosted_model_fields", "key_source",
    }

    assert lexi.providers.add("mine", "https://x.example/v1", "m1", api_key="sk-abcdefghij") == {
        "added": "mine"
    }
    mine = next(p for p in lexi.providers.list() if p["name"] == "mine")
    assert mine["api"] == "chat-completions" and mine["key_source"] == "file"
    assert lexi.models.default("mine")["default_model"] == "m1"
    assert (home / "providers" / "mine" / "api-key").read_text().strip() == "sk-abcdefghij"

    assert lexi.providers.show_key("mine")["key"] == "sk-a*****ghij"
    assert lexi.providers.show_key("mine", reveal=True)["key"] == "sk-abcdefghij"
    assert lexi.providers.rename("mine", "yours") == {"renamed": "mine", "to": "yours"}
    assert lexi.providers.remove_key("yours") == {"removed": True}
    assert lexi.providers.show_key("yours") == {"provider": "yours", "source": None, "key": None}
    lexi.providers.set_key("yours", "k2")
    assert lexi.providers.show_key("yours", reveal=True)["key"] == "k2"
    assert lexi.providers.remove("yours") == {"removed": "yours"}
    assert "yours" not in [p["name"] for p in lexi.providers.list()]

    with pytest.raises(lexi.LexiError, match="unknown provider"):
        lexi.providers.show_key("nope")
    with pytest.raises(lexi.LexiError):
        lexi.providers.add("bad", "https://x", "m", api="no-such-api")


def test_provider_status_and_hosted_models(home):
    m = Mock()
    try:
        m.routes[("GET", "/models")] = ok({"data": [{"id": "m-a"}, {"id": "m-b"}]})
        lexi.providers.add("mock", m.url, "m-a", api_key="k")
        assert lexi.providers.status("mock") == {
            "provider": "mock", "status": "ready", "source": "file",
        }
        assert [x["id"] for x in lexi.models.hosted("mock")] == ["m-a", "m-b"]
        lexi.providers.remove_key("mock")
        assert lexi.providers.status("mock")["status"] == "no api-key"
    finally:
        m.server.shutdown()


def test_models_and_aliases(home):
    lexi.providers.add("p", "https://x.example", "base")
    assert lexi.models.add("p", "big", name="Big", max_tokens=8000, tier="pro") == {
        "provider": "p", "model": "big", "existed": False,
    }
    assert lexi.models.set("p", "big", description="the big one")["existed"] is True
    lexi.models.add("p", "small", default=True)
    models = {m["id"]: m for m in lexi.models.list("p")}
    assert models["big"]["name"] == "Big"
    assert models["big"]["description"] == "the big one"
    assert models["big"]["metadata"]["tier"] == "pro"
    assert models["small"]["default"] is True
    assert lexi.models.default("p") == {"provider": "p", "default_model": "small"}
    lexi.models.set_default("p", "big")
    assert lexi.models.default("p")["default_model"] == "big"

    lexi.models.set_alias("p", "z", "small")
    lexi.models.set_alias("p", "a", "big")
    assert lexi.models.aliases("p") == [{"alias": "a", "model": "big"}, {"alias": "z", "model": "small"}]
    assert lexi.models.remove_alias("p", "a") == {"provider": "p", "removed": "a"}
    assert lexi.models.remove("p", "small") == {"provider": "p", "removed": "small"}
    assert [m["id"] for m in lexi.models.list("p")] == ["big"]

    with pytest.raises(lexi.LexiError, match="unknown provider"):
        lexi.models.list("nope")


def test_prompts(home):
    saved = lexi.prompts.save("review", "Review this", temperature=0.1)
    assert saved == {
        "name": "review", "active": False, "prompt": "Review this",
        "max_tokens": 256, "temperature": 0.1, "role": "user",
    }
    # Updating keeps the fields not given.
    assert lexi.prompts.save("review", max_tokens=99)["temperature"] == 0.1
    assert lexi.prompts.get("review")["max_tokens"] == 99

    assert lexi.prompts.activate("review") == {"active": "review"}
    first = lexi.prompts.list()[0]
    assert first["name"] == lexi.prompts.ACTIVE and first["active"] is True
    assert first["prompt"] == "Review this"

    assert lexi.prompts.remove("review") == {"removed": "review"}
    with pytest.raises(lexi.LexiError, match='prompt "review" not found'):
        lexi.prompts.get("review")
    with pytest.raises(lexi.LexiError, match="prompt text"):
        lexi.prompts.save("empty")


def test_config(home):
    settings = lexi.config.list()
    assert settings["cli_name"] == "lexi"  # a default
    assert lexi.config.set("edit_mode", "emacs") == {"key": "edit_mode", "value": "emacs"}
    lexi.config.set("count", 3)
    assert lexi.config.get("edit_mode") == "emacs"
    assert lexi.config.get("count") == 3
    assert lexi.config.remove("count") == {"removed": "count"}
    with pytest.raises(lexi.LexiError, match="not found"):
        lexi.config.get("count")


def test_api_definitions(home, tmp_path):
    defs = {d["name"]: d for d in lexi.apis.list()}
    assert defs["messages"]["builtin"] is True
    assert lexi.apis.show("messages")["request"]["path"] == "/messages"
    assert lexi.apis.validate("messages") == {"name": "messages", "valid": True, "errors": []}

    assert lexi.apis.create("mine", template="messages", path="/v2/messages") == {"created": "mine"}
    assert lexi.apis.show("mine")["request"]["path"] == "/v2/messages"
    with pytest.raises(lexi.LexiError, match="already exists"):
        lexi.apis.create("mine")
    lexi.apis.edit("mine", text_path="content[0].text", metadata={"stop": "stop_reason"})
    edited = lexi.apis.show("mine")
    assert edited["response"]["text_extract"]["text_path"] == "content[0].text"
    assert edited["response"]["metadata"]["stop"] == "stop_reason"

    preview = lexi.apis.preview("mine", prompt="hi", model="mx")
    assert preview["valid"] is True and preview["body"]["model"] == "mx"

    bad = tmp_path / "bad.yaml"
    bad.write_text("name: bad\nrequest: {}\n")
    saved = lexi.apis.save("bad", bad)
    assert saved["saved"] == "bad" and saved["valid"] is False and saved["errors"]
    with pytest.raises(lexi.LexiError, match="shipped"):
        lexi.apis.save("messages", "name: messages\n")

    assert lexi.apis.remove("mine") == {"removed": "mine"}
    assert "mine" not in [d["name"] for d in lexi.apis.list()]
