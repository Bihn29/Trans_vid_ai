"""Deterministic echo worker entry point."""

from __future__ import annotations

import sys
import time
from pathlib import Path
from typing import cast

if __package__ in {None, ""}:
    sys.path.insert(0, str(Path(__file__).resolve().parents[2]))

from workers.common.protocol import (
    PROTOCOL_VERSION,
    JsonObject,
    JsonValue,
    ProtocolError,
    emit_event,
    read_request,
)

_FALLBACK_REQUEST_ID = "00000000-0000-4000-8000-000000000000"


def _request_id(request: JsonObject | None) -> str:
    value = request.get("request_id") if request is not None else None
    return value if isinstance(value, str) else _FALLBACK_REQUEST_ID


def _progress(request_id: str, progress: int, message: str) -> JsonObject:
    return {
        "protocol_version": PROTOCOL_VERSION,
        "request_id": request_id,
        "event": "progress",
        "progress": progress,
        "message": message,
    }


def _completed(request_id: str, metrics: JsonObject) -> JsonObject:
    return {
        "protocol_version": PROTOCOL_VERSION,
        "request_id": request_id,
        "event": "completed",
        "artifacts": [],
        "metrics": metrics,
        "warnings": [],
    }


def _failed(request_id: str, error_code: str, safe_message: str) -> JsonObject:
    return {
        "protocol_version": PROTOCOL_VERSION,
        "request_id": request_id,
        "event": "failed",
        "error_code": error_code,
        "safe_message": safe_message,
    }


def _sleep_duration(config_value: JsonValue) -> float:
    if not isinstance(config_value, dict):
        return 5.0
    milliseconds = config_value.get("delay_ms", 5000)
    if isinstance(milliseconds, bool) or not isinstance(milliseconds, int):
        raise ProtocolError("INVALID_DELAY", "Thời gian chờ thử nghiệm không hợp lệ.")
    if milliseconds < 0 or milliseconds > 60_000:
        raise ProtocolError("INVALID_DELAY", "Thời gian chờ thử nghiệm không hợp lệ.")
    return milliseconds / 1000


def handle(request: JsonObject) -> None:
    request_id = cast(str, request["request_id"])
    action = cast(str, request["action"])

    emit_event(_progress(request_id, 0, "Echo worker started"))

    if action == "fail":
        emit_event(
            _failed(
                request_id,
                "ECHO_REQUESTED_FAILURE",
                "Tác vụ kiểm tra đã trả về lỗi theo yêu cầu.",
            )
        )
        return

    if action == "sleep":
        time.sleep(_sleep_duration(request.get("config")))
    elif action != "echo":
        emit_event(_failed(request_id, "UNSUPPORTED_ACTION", "Bộ máy không hỗ trợ tác vụ này."))
        return

    emit_event(_progress(request_id, 100, "Echo worker completed"))
    emit_event(
        _completed(
            request_id,
            {
                "worker": "echo",
                "echo": request.get("input", {}),
            },
        )
    )


def main() -> int:
    request: JsonObject | None = None
    try:
        request = read_request()
        handle(request)
        return 0
    except ProtocolError as error:
        emit_event(_failed(_request_id(request), error.error_code, error.safe_message))
        return 0
    except (OSError, ValueError, TypeError):
        emit_event(
            _failed(
                _request_id(request),
                "ECHO_INTERNAL_ERROR",
                "Bộ máy kiểm tra gặp lỗi nội bộ.",
            )
        )
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
