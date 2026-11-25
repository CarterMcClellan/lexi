"""The runtime bindings (lexi-host from Python) against a local mock provider.

Each test gets its own ``$LEXI_HOME`` whose providers point at an in-process
HTTP server, so nothing reaches a real provider.
"""

import base64
import json

import pytest

import lexi
from conftest import ok


RESPONSE = {
    "model": "m",
    "status": "completed",
    "usage": {"input_tokens": 1, "output_tokens": 1, "total_tokens": 2},
    "output": [
        {
            "type": "message",
            "content": [
                {
                    "type": "output_text",
                    "text": "Answer.",
                    "annotations": [
                        {"type": "url_citation", "url": "https://a.example", "title": "A"}
                    ],
                }
            ],
        }
    ],
}

SSE = (
    "event: response.web_search_call.searching\ndata: {}\n\n"
    'event: response.output_text.delta\ndata: {"delta":"Ans"}\n\n'
    'event: response.output_text.delta\ndata: {"delta":"wer."}\n\n'
    "event: response.completed\ndata: "
    + json.dumps({"type": "response.completed", "response": RESPONSE})
    + "\n\n"
)


def test_respond_returns_the_cli_json_shape(mock):
    mock.routes[("POST", "/responses")] = ok(RESPONSE)
    r = lexi.respond("p", "hi")
    assert r["provider"] == "p"
    assert r["model"] == "m"
    assert r["text"] == "Answer."
    assert r["metadata"]["total_tokens"] == 2
    assert "sources" not in r and "raw" not in r
    body = mock.bodies()[0]
    assert body["model"] == "m"
    assert "hi" in json.dumps(body["input"])

    r = lexi.respond("p", "hi", cite=True, raw=True)
    assert r["sources"] == ["[1] https://a.example — A"]
    assert r["raw"]["status"] == "completed"


def test_saved_prompts_role_and_body_options(mock):
    mock.routes[("POST", "/responses")] = ok(RESPONSE)
    prompts = mock.home / "prompts"
    prompts.mkdir()
    (prompts / "greet.json").write_text(json.dumps({"prompt": "hello there", "role": "user"}))
    (prompts / "$$.json").write_text(json.dumps({"prompt": "the active one"}))

    lexi.respond("p", prompt="greet")
    lexi.respond("p")
    lexi.respond(
        "p",
        "x",
        params={"max_output_tokens": 50},
        set={"temperature": 0.25},
        unset=["max_output_tokens"],
    )
    first, active, opts = mock.bodies()
    assert "hello there" in json.dumps(first["input"])
    assert "the active one" in json.dumps(active["input"])
    assert opts["temperature"] == 0.25
    assert "max_output_tokens" not in opts  # unset beats params

    with pytest.raises(lexi.LexiError, match='prompt "nope" not found'):
        lexi.respond("p", prompt="nope")
    with pytest.raises(ValueError, match="not both"):
        lexi.respond("p", "x", prompt="greet")


def test_prepare_composes_without_sending(mock):
    out = lexi.prepare("p", "hi", set=[("temperature", 0.5)], mode="stream")
    assert out["provider"] == "p" and out["model"] == "m"
    assert out["body"]["stream"] is True
    assert out["body"]["temperature"] == 0.5
    assert mock.requests == []


def test_stream_yields_the_ndjson_events(mock):
    mock.routes[("POST", "/responses")] = lambda _b: (200, "text/event-stream", SSE.encode())
    events = list(lexi.stream("p", "hi", cite=True))
    kinds = [e["event"] for e in events]
    assert kinds[0] == "provider_start"
    assert kinds[-1] == "provider_done"
    assert "activity" in kinds
    assert "".join(e["text"] for e in events if e["event"] == "delta") == "Answer."
    done = events[-1]
    assert done["text"] == "Answer."
    assert done["model"] == "m"
    assert done["sources"] == ["[1] https://a.example — A"]
    assert mock.bodies()[0]["stream"] is True


def test_stream_failure_raises(mock):
    mock.routes[("POST", "/responses")] = lambda _b: (500, "application/json", b'{"error":"boom"}')
    it = lexi.stream("p", "hi")
    assert next(it)["event"] == "provider_start"
    with pytest.raises(lexi.LexiError, match="500"):
        next(it)
    with pytest.raises(StopIteration):
        next(it)


def _job_routes(mock):
    polls = {"n": 0}

    def poll(_b):
        polls["n"] += 1
        body = {"id": "resp_1", "status": "in_progress"} if polls["n"] == 1 else dict(RESPONSE, id="resp_1")
        return 200, "application/json", json.dumps(body).encode()

    mock.routes[("POST", "/responses")] = ok({"id": "resp_1", "status": "queued"})
    mock.routes[("GET", "/responses/resp_1")] = poll


def test_submit_then_status_then_wait(mock):
    _job_routes(mock)
    job = lexi.submit("p", "go")
    assert job["id"] == "resp_1"
    assert job["provider"] == "p"
    assert job["done"] is False
    assert mock.bodies()[0]["background"] is True

    st = lexi.jobs.status("p", "resp_1")
    assert st["state"] == "running" and "text" not in st
    done = lexi.jobs.wait("p", "resp_1", interval=0.01, timeout=5)
    assert done["state"] == "succeeded"
    assert done["text"] == "Answer."


def test_background_respond_polls_to_completion(mock):
    _job_routes(mock)
    r = lexi.respond("p", "go", background=True)
    assert r["text"] == "Answer."


def test_generate_and_upload(mock, tmp_path):
    png = base64.b64encode(b"\x89PNG fake").decode()
    mock.routes[("POST", "/images/generations")] = ok(
        {"data": [{"b64_json": png, "revised_prompt": "a refined cat"}]}
    )
    out = lexi.generate("img", "a cat", out_dir=tmp_path / "out")
    assert out["provider"] == "img"
    [art] = out["artifacts"]
    assert art["revised_prompt"] == "a refined cat"
    with open(art["path"], "rb") as f:
        assert f.read() == b"\x89PNG fake"

    mock.routes[("POST", "/files")] = ok({"id": "file-1"})
    f = tmp_path / "cat.png"
    f.write_bytes(b"img")
    assert lexi.upload("p", f) == {"provider": "p", "file_id": "file-1"}


def test_errors_and_first_run_seeding(mock):
    with pytest.raises(lexi.LexiError, match="unknown provider"):
        lexi.respond("nope", "hi")
    assert issubclass(lexi.LexiError, Exception)
    # The first call seeded the missing defaults (like the CLI), keeping ours.
    assert (mock.home / "aliases.json").exists()
    assert "img:" in (mock.home / "providers" / "providers-config.yaml").read_text()


def test_engine_functions_are_still_top_level():
    assert callable(lexi.compose_request_body)
    assert callable(lexi.validate_definition)
    assert "respond" in lexi.__all__ and "compose_request_body" in lexi.__all__


def test_one_reply_with_text_and_image(mock, tmp_path):
    png = base64.b64encode(b"\x89PNG fake").decode()
    reply = {
        "output": [
            {"type": "image_generation_call", "status": "completed", "result": png},
            {"type": "message", "content": [{"type": "output_text", "text": "Here's your fox."}]},
        ]
    }
    mock.routes[("POST", "/responses")] = ok(reply)
    r = lexi.respond("p", "draw a fox", tools=["image_generation"], out_dir=tmp_path)
    assert r["text"] == "Here's your fox." and r["api"] == "responses"
    [art] = r["artifacts"]
    assert art["path"].endswith("draw-a-fox-1.png") and art["type"] == "image"
    with open(art["path"], "rb") as f:
        assert f.read() == b"\x89PNG fake"

    # Streamed: an `artifact` event arrives before provider_done.
    done = {"type": "response.completed", "response": reply}
    sse = (
        'event: response.output_text.delta\ndata: {"delta":"Here\'s your fox."}\n\n'
        "event: response.completed\ndata: " + json.dumps(done) + "\n\n"
    )
    mock.routes[("POST", "/responses")] = lambda _b: (200, "text/event-stream", sse.encode())
    events = list(lexi.stream("p", "draw a fox", tools=["image_generation"], out_dir=tmp_path))
    kinds = [e["event"] for e in events]
    assert kinds[-2:] == ["artifact", "provider_done"]
    assert events[-2]["path"].endswith("draw-a-fox-2.png")
    assert events[-1]["artifacts"][0]["path"] == events[-2]["path"]


def test_output_image_picks_the_image_api(mock, tmp_path):
    mock.routes[("POST", "/images/generations")] = ok(
        {"data": [{"b64_json": base64.b64encode(b"img").decode()}]}
    )
    r = lexi.respond("img", "a fox", output="image", out_dir=tmp_path)
    assert r["api"] == "image-generations" and r["text"] == ""
    assert r["artifacts"][0]["path"].endswith("a-fox-1.png")


def test_embed(mock, tmp_path):
    mock.routes[("POST", "/embeddings")] = ok({
        "data": [{"index": 1, "embedding": [0.3, 0.4]}, {"index": 0, "embedding": [0.1, 0.2]}],
        "usage": {"prompt_tokens": 2, "total_tokens": 2},
    })
    (mock.home / "providers" / "providers-config.yaml").write_text(
        f"providers:\n  nv:\n    template: nvidia\n    url: {mock.url}\n    api_key: k\n"
    )
    r = lexi.embed("nv", ["a", "b"], set={"input_type": "passage"})
    assert r["api"] == "embeddings" and r["model"] == "nvidia/nemotron-3-embed-1b"
    assert r["embeddings"] == [[0.1, 0.2], [0.3, 0.4]]
    assert (r["count"], r["dimensions"]) == (2, 2)
    sent = mock.bodies()[-1]
    assert sent["input"] == ["a", "b"] and sent["input_type"] == "passage"
    assert lexi.embed("nv", "one text")["count"] == 2  # (the mock always returns two)
    with pytest.raises(lexi.LexiError, match="doesn't produce vector"):
        lexi.embed("nv", "x", model="meta/llama-3.1-70b-instruct")


def test_named_model_picks_its_output(mock, tmp_path):
    # No output= given: the image model's tags pick the image API.
    mock.routes[("POST", "/images/generations")] = ok({"data": [{"b64_json": base64.b64encode(b"i").decode()}]})
    (mock.home / "providers" / "providers-config.yaml").write_text(
        f"providers:\n  oa:\n    template: openai\n    url: {mock.url}\n    api_key: k\n"
    )
    r = lexi.respond("oa", "a fox", model="gpt-image-9", out_dir=tmp_path)
    assert r["api"] == "image-generations" and len(r["artifacts"]) == 1
    assert lexi.models.resolve("oa", "gpt-image-9")["api"] == "image-generations"
    with pytest.raises(lexi.LexiError, match="doesn't produce text"):
        lexi.models.resolve("oa", "gpt-image-9", output="text")


def test_timeout_limits_one_request(mock):
    import time

    def slow(_body):
        time.sleep(2)
        return 200, "application/json", json.dumps(RESPONSE).encode()

    mock.routes[("POST", "/responses")] = slow
    started = time.monotonic()
    with pytest.raises(lexi.LexiError, match="within 1 seconds.*timeout given for this request"):
        lexi.respond("p", "hi", timeout=1)
    assert time.monotonic() - started < 1.9
    with pytest.raises(lexi.LexiError, match="sent nothing for 1 seconds"):
        list(lexi.stream("p", "hi", timeout=1))
    # 0 means no limit for this request; config.json is left alone.
    assert lexi.respond("p", "hi", timeout=0)["text"]
    cfg = mock.home / "config.json"
    assert not cfg.exists() or "request_timeout" not in cfg.read_text()
    with pytest.raises(ValueError, match="whole number of seconds"):
        lexi.respond("p", "hi", timeout=-1)
