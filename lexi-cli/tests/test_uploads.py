"""Tests for Files-API upload interpretation (uploads)."""

from __future__ import annotations

from lexi_cli import uploads

_FILES = {"upload": {"method": "POST", "path": "/files", "purpose_field": "purpose"}, "id_path": "id"}


def test_upload_path():
    assert uploads.upload_path(_FILES) == "/files"
    assert uploads.upload_path(None) == ""
    assert uploads.upload_path({}) == ""


def test_upload_purpose_field():
    assert uploads.upload_purpose_field(_FILES) == "purpose"
    assert uploads.upload_purpose_field({"upload": {"path": "/files"}}) is None
    assert uploads.upload_purpose_field(None) is None


def test_read_upload():
    assert uploads.read_upload({"id": "file-1", "object": "file"}, _FILES) == {"id": "file-1"}


def test_read_upload_custom_id_path():
    cfg = {"upload": {"path": "/files"}, "id_path": "file.name"}
    assert uploads.read_upload({"file": {"name": "files/abc"}}, cfg) == {"id": "files/abc"}


def test_read_upload_without_cfg():
    assert uploads.read_upload({"id": "x"}, None) == {"id": None}


def test_read_upload_missing_id():
    assert uploads.read_upload({"object": "file"}, _FILES) == {"id": None}
