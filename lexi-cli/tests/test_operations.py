"""Tests for the async operation lifecycle (operations)."""

from __future__ import annotations

from lexi_cli import operations

_OP = {
    "id_path": "id",
    "status_path": "status",
    "poll": {"method": "GET", "path_template": "/responses/{id}"},
    "status_map": {
        "pending": ["queued"],
        "running": ["in_progress"],
        "succeeded": ["completed"],
        "failed": ["failed", "cancelled", "incomplete"],
    },
}


def test_classify_known_and_unknown():
    sm = _OP["status_map"]
    assert operations.classify_status("queued", sm) == "pending"
    assert operations.classify_status("in_progress", sm) == "running"
    assert operations.classify_status("completed", sm) == "succeeded"
    assert operations.classify_status("cancelled", sm) == "failed"
    assert operations.classify_status("weird", sm) == "unknown"
    assert operations.classify_status(None, sm) == "unknown"


def test_read_operation_from_submit():
    op = operations.read_operation({"id": "resp_1", "status": "queued"}, _OP)
    assert op == {"id": "resp_1", "status": "queued", "state": "pending", "done": False}


def test_read_operation_terminal():
    op = operations.read_operation({"id": "resp_1", "status": "completed"}, _OP)
    assert op == {"id": "resp_1", "status": "completed", "state": "succeeded", "done": True}
    assert operations.read_operation({"status": "failed"}, _OP)["done"] is True


def test_read_operation_without_cfg():
    assert operations.read_operation({"id": "x"}, None) == {
        "id": None,
        "status": None,
        "state": "unknown",
        "done": False,
    }


def test_poll_path_substitutes_id():
    assert operations.poll_path(_OP, "resp_1") == "/responses/resp_1"


def test_is_terminal():
    assert operations.is_terminal("succeeded")
    assert operations.is_terminal("failed")
    assert not operations.is_terminal("running")
    assert not operations.is_terminal("pending")
