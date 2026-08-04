from __future__ import annotations

import hashlib
import json
import os
import sys
from pathlib import Path
from typing import cast

if __package__ in {None, ""}:
    sys.path.insert(0, str(Path(__file__).resolve().parents[2]))

from jsonschema import Draft202012Validator

from workers.common.protocol import (
    PROTOCOL_VERSION,
    JsonObject,
    JsonValue,
    ProtocolError,
    emit_event,
    read_request,
)
from workers.separation.contract import EnergyMaskProvider

ROOT = Path(__file__).resolve().parents[2]
with (ROOT / "schemas" / "separation-request.schema.json").open(encoding="utf-8") as stream:
    VALIDATOR = Draft202012Validator(json.load(stream))
FALLBACK = "00000000-0000-4000-8000-000000000000"
MAX_SOURCE_BYTES = 512 * 1024 * 1024


def event(request_id: str, event_name: str, **values: JsonValue) -> JsonObject:
    return {
        "protocol_version": PROTOCOL_VERSION,
        "request_id": request_id,
        "event": event_name,
        **values,
    }


def contained_existing(relative: str) -> Path:
    root = Path.cwd().resolve(strict=True)
    parts = relative.split("/")
    if any(part in {"", ".", ".."} for part in parts):
        raise ProtocolError("UNSAFE_PATH", "Đường dẫn âm thanh không hợp lệ.")
    candidate = root / Path(*parts)
    metadata = candidate.lstat()
    resolved = candidate.resolve(strict=True)
    if candidate.is_symlink() or not candidate.is_file() or not resolved.is_relative_to(root):
        raise ProtocolError("UNSAFE_PATH", "Đường dẫn âm thanh không hợp lệ.")
    if metadata.st_size <= 0 or metadata.st_size > MAX_SOURCE_BYTES:
        raise ProtocolError("INVALID_AUDIO", "Âm thanh nguồn không hợp lệ.")
    return resolved


def write_new(relative: str, data: bytes) -> None:
    root = Path.cwd().resolve(strict=True)
    destination = root / Path(*relative.split("/"))
    parent = destination.parent.resolve(strict=True)
    if not parent.is_relative_to(root) or parent.is_symlink():
        raise ProtocolError("UNSAFE_PATH", "Đường dẫn âm thanh không hợp lệ.")
    with destination.open("xb") as output:
        output.write(data)
        output.flush()
        os.fsync(output.fileno())


def artifact(kind: str, relative: str, data: bytes, duration: int, rate: int) -> JsonObject:
    return {
        "type": kind,
        "relative_path": relative,
        "sha256": hashlib.sha256(data).hexdigest(),
        "size_bytes": len(data),
        "metadata": {
            "duration_ms": duration,
            "sample_rate": rate,
            "channels": 1,
            "bits_per_sample": 16,
            "engine_id": "energy-mask-v1",
            "separation_mode": "separated",
        },
    }


def handle(request: JsonObject) -> None:
    request_id = cast(str, request["request_id"])
    if request.get("action") != "separate_audio":
        raise ProtocolError("UNSUPPORTED_ACTION", "Tác vụ tách âm không được hỗ trợ.")
    input_value = request.get("input")
    payload = input_value.get("separation") if isinstance(input_value, dict) else None
    if not isinstance(payload, dict) or list(VALIDATOR.iter_errors(payload)):
        raise ProtocolError("INVALID_SEPARATION_REQUEST", "Yêu cầu tách âm không hợp lệ.")
    source = contained_existing(cast(str, payload["source_relative_path"]))
    emit_event(event(request_id, "progress", progress=0, message="Separation started"))
    result = EnergyMaskProvider().separate(
        source.read_bytes(), float(cast(float, payload["energy_threshold"]))
    )
    vocals_path = f"audio/vocals/separation-{request_id}.wav"
    background_path = f"audio/background/separation-{request_id}.wav"
    write_new(vocals_path, result.vocals)
    try:
        write_new(background_path, result.background)
    except (OSError, ProtocolError):
        (Path.cwd() / Path(*vocals_path.split("/"))).unlink(missing_ok=True)
        raise
    emit_event(event(request_id, "progress", progress=100, message="Separation completed"))
    emit_event(
        event(
            request_id,
            "completed",
            artifacts=[
                artifact(
                    "vocals", vocals_path, result.vocals, result.duration_ms, result.sample_rate
                ),
                artifact(
                    "background",
                    background_path,
                    result.background,
                    result.duration_ms,
                    result.sample_rate,
                ),
            ],
            metrics={"engine_id": "energy-mask-v1", "sends_data_off_device": False},
            warnings=[],
        )
    )


def main() -> int:
    request: JsonObject | None = None
    try:
        request = read_request()
        handle(request)
        return 0
    except ProtocolError as error:
        emit_event(
            event(
                cast(str, request.get("request_id", FALLBACK)) if request else FALLBACK,
                "failed",
                error_code=error.error_code,
                safe_message=error.safe_message,
            )
        )
        return 0
    except (OSError, ValueError, TypeError):
        emit_event(
            event(
                cast(str, request.get("request_id", FALLBACK)) if request else FALLBACK,
                "failed",
                error_code="SEPARATION_INTERNAL_ERROR",
                safe_message="Bộ máy tách âm gặp lỗi nội bộ.",
            )
        )
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
