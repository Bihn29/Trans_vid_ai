"""Versioned, size-bounded JSON Lines protocol shared by Python workers."""

from __future__ import annotations

import json
import sys
from pathlib import Path, PurePosixPath
from typing import TextIO, TypeAlias, cast

from jsonschema import Draft202012Validator

JsonScalar: TypeAlias = None | bool | int | float | str
JsonValue: TypeAlias = JsonScalar | list["JsonValue"] | dict[str, "JsonValue"]
JsonObject: TypeAlias = dict[str, JsonValue]

PROTOCOL_VERSION = 1
MAX_REQUEST_BYTES = 1024 * 1024
_REPOSITORY_ROOT = Path(__file__).resolve().parents[2]
_SCHEMA_DIRECTORY = _REPOSITORY_ROOT / "schemas"


class ProtocolError(Exception):
    """A safe protocol failure with a stable machine-readable code."""

    def __init__(self, error_code: str, safe_message: str) -> None:
        super().__init__(safe_message)
        self.error_code = error_code
        self.safe_message = safe_message


def _load_schema(filename: str) -> JsonObject:
    schema_path = _SCHEMA_DIRECTORY / filename
    with schema_path.open(encoding="utf-8") as schema_file:
        parsed = cast(JsonValue, json.load(schema_file))
    if not isinstance(parsed, dict):
        raise RuntimeError(f"Schema {filename} must be an object")
    return parsed


_REQUEST_VALIDATOR = Draft202012Validator(_load_schema("worker-request.schema.json"))
_RESPONSE_VALIDATOR = Draft202012Validator(_load_schema("worker-response.schema.json"))


def _validate(validator: Draft202012Validator, payload: JsonObject, code: str) -> None:
    errors = sorted(validator.iter_errors(payload), key=lambda error: list(error.absolute_path))
    if errors:
        raise ProtocolError(code, "Dữ liệu giao tiếp với bộ máy không hợp lệ.")


def is_safe_relative_path(value: str) -> bool:
    if not value or "\\" in value or ":" in value:
        return False
    path = PurePosixPath(value)
    return not path.is_absolute() and all(part not in {"", ".", ".."} for part in path.parts)


def validate_request(payload: JsonObject) -> None:
    _validate(_REQUEST_VALIDATOR, payload, "INVALID_WORKER_REQUEST")
    output_directory = payload.get("output_directory")
    if not isinstance(output_directory, str) or not is_safe_relative_path(output_directory):
        raise ProtocolError(
            "INVALID_OUTPUT_DIRECTORY",
            "Thư mục đầu ra của tác vụ không hợp lệ.",
        )


def validate_event(payload: JsonObject) -> None:
    _validate(_RESPONSE_VALIDATOR, payload, "INVALID_WORKER_RESPONSE")
    artifacts = payload.get("artifacts", [])
    if not isinstance(artifacts, list):
        return
    for artifact in artifacts:
        if not isinstance(artifact, dict):
            raise ProtocolError("INVALID_ARTIFACT", "Thông tin tệp đầu ra không hợp lệ.")
        relative_path = artifact.get("relative_path")
        if not isinstance(relative_path, str) or not is_safe_relative_path(relative_path):
            raise ProtocolError("INVALID_ARTIFACT_PATH", "Đường dẫn tệp đầu ra không hợp lệ.")


def read_request(stream: TextIO = sys.stdin) -> JsonObject:
    line = stream.readline(MAX_REQUEST_BYTES + 1)
    if not line:
        raise ProtocolError("EMPTY_REQUEST", "Bộ máy không nhận được yêu cầu xử lý.")
    if len(line.encode("utf-8")) > MAX_REQUEST_BYTES or not line.endswith("\n"):
        raise ProtocolError("REQUEST_TOO_LARGE", "Yêu cầu xử lý vượt quá giới hạn cho phép.")
    try:
        parsed = cast(JsonValue, json.loads(line))
    except json.JSONDecodeError as error:
        raise ProtocolError("INVALID_JSON", "Yêu cầu xử lý không phải JSON hợp lệ.") from error
    if not isinstance(parsed, dict):
        raise ProtocolError("INVALID_WORKER_REQUEST", "Yêu cầu xử lý phải là một object.")
    validate_request(parsed)
    return parsed


def emit_event(payload: JsonObject, stream: TextIO = sys.stdout) -> None:
    validate_event(payload)
    serialized = json.dumps(payload, ensure_ascii=False, separators=(",", ":"))
    stream.write(serialized)
    stream.write("\n")
    stream.flush()

