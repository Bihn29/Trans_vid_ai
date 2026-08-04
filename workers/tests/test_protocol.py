from __future__ import annotations

import io
import json
from typing import cast
from uuid import uuid4

import pytest

from workers.common.protocol import (
    JsonObject,
    ProtocolError,
    emit_event,
    is_safe_relative_path,
    read_request,
    validate_event,
    validate_request,
)


def valid_request() -> JsonObject:
    return {
        "protocol_version": 1,
        "request_id": str(uuid4()),
        "action": "echo",
        "project_id": str(uuid4()),
        "input": {"text": "xin chào"},
        "config": {},
        "output_directory": "metadata/echo",
    }


def test_request_schema_accepts_protocol_v1() -> None:
    validate_request(valid_request())


@pytest.mark.parametrize(
    "path",
    ["../escape", "metadata/../escape", "C:/escape", "/absolute", "metadata\\escape"],
)
def test_request_rejects_unsafe_output_directory(path: str) -> None:
    request = valid_request()
    request["output_directory"] = path

    with pytest.raises(ProtocolError):
        validate_request(request)


def test_schema_rejects_unknown_fields() -> None:
    request = valid_request()
    request["unexpected"] = True

    with pytest.raises(ProtocolError, match="không hợp lệ"):
        validate_request(request)


def test_read_request_requires_newline_terminated_object() -> None:
    encoded = json.dumps(valid_request(), ensure_ascii=False) + "\n"
    parsed = read_request(io.StringIO(encoded))

    assert parsed["action"] == "echo"


def test_emit_event_writes_one_compact_json_line() -> None:
    request_id = cast(str, valid_request()["request_id"])
    event: JsonObject = {
        "protocol_version": 1,
        "request_id": request_id,
        "event": "progress",
        "progress": 42,
        "message": "Processing",
    }
    output = io.StringIO()

    emit_event(event, output)

    assert output.getvalue().endswith("\n")
    assert "\n" not in output.getvalue()[:-1]
    validate_event(cast(JsonObject, json.loads(output.getvalue())))


def test_relative_path_policy() -> None:
    assert is_safe_relative_path("audio/tts/segment.wav")
    assert not is_safe_relative_path("audio/../../secret")

