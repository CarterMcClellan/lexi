"""Files-API uploads: turn a local file into a provider file handle.

The upload itself is a multipart HTTP POST done by the host (CLI/GUI). This
module interprets an API def's ``files`` block — where to POST and how to read
the resulting file id from the upload response — so the handle can then be used
as an :class:`~lexi_cli.message_ir.ImagePart` ``file_id`` source. Pure: no I/O.

See ``docs/spec/uploads-v0.md``.
"""

from __future__ import annotations

from collections.abc import Mapping
from typing import Any

from lexi_cli.api_compose import get_path


def upload_path(files_cfg: Mapping[str, Any] | None) -> str:
    """Return the Files-API upload path (e.g. ``/files``), or ``""`` if absent."""
    if not files_cfg:
        return ""
    return (files_cfg.get("upload") or {}).get("path", "")


def upload_purpose_field(files_cfg: Mapping[str, Any] | None) -> str | None:
    """Return the multipart field name carrying the upload purpose, if any."""
    if not files_cfg:
        return None
    return (files_cfg.get("upload") or {}).get("purpose_field")


def read_upload(upload_response: Any, files_cfg: Mapping[str, Any] | None) -> dict:
    """Read the file handle from an upload response.

    Returns ``{"id": <file id or None>}``. The id is read from ``files.id_path``
    (default ``id``).
    """
    if not files_cfg:
        return {"id": None}
    return {"id": get_path(upload_response, files_cfg.get("id_path", "id"))}
