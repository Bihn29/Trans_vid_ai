from __future__ import annotations

import hashlib
import json
import os
import sys
from pathlib import Path
from typing import cast

if __package__ in {None, ""}:
    sys.path.insert(0, str(Path(__file__).resolve().parents[2]))

from jsonschema import Draft202012Validator, FormatChecker

from workers.common.protocol import (
    PROTOCOL_VERSION,
    JsonObject,
    JsonValue,
    ProtocolError,
    emit_event,
    read_request,
)
from workers.tts.contract import TtsProvider, synthesize_with_retry, validate_wav
from workers.tts.providers.openai_compatible import OpenAiCompatibleTtsAdapter

ROOT = Path(__file__).resolve().parents[2]
with (ROOT / "schemas" / "tts-request.schema.json").open(encoding="utf-8") as stream:
    VALIDATOR = Draft202012Validator(json.load(stream), format_checker=FormatChecker())
with (ROOT / "schemas" / "tts-batch-request.schema.json").open(encoding="utf-8") as stream:
    BATCH_VALIDATOR = Draft202012Validator(json.load(stream), format_checker=FormatChecker())
FALLBACK = "00000000-0000-4000-8000-000000000000"


def event(request_id: str, event_name: str, **values: JsonValue) -> JsonObject:
    return {
        "protocol_version": PROTOCOL_VERSION,
        "request_id": request_id,
        "event": event_name,
        **values,
    }


def resolve(config: JsonObject) -> TtsProvider:
    if config.get("provider_id") != "openai-compatible" or config.get("cloud_consent") is not True:
        raise ProtocolError(
            "CLOUD_CONSENT_REQUIRED", "Cần xác nhận trước khi gửi bản dịch ra ngoài thiết bị."
        )
    values = [config.get(key) for key in ("endpoint", "model", "api_key")]
    if not all(isinstance(value, str) for value in values):
        raise ProtocolError("INVALID_PROVIDER_CONFIG", "Cấu hình TTS không hợp lệ.")
    return OpenAiCompatibleTtsAdapter(
        cast(str, values[0]), cast(str, values[1]), cast(str, values[2])
    )


def handle_local_batch(request: JsonObject, data: JsonObject, config: JsonObject) -> None:
    rid = cast(str, request["request_id"])
    payload = data.get("tts_batch")
    if (
        config.get("provider_id") != "local-melo"
        or config.get("cloud_consent") is not False
        or not isinstance(payload, dict)
        or list(BATCH_VALIDATOR.iter_errors(payload))
    ):
        raise ProtocolError("INVALID_TTS_REQUEST", "Yêu cầu giọng Việt cục bộ không hợp lệ.")
    model_path = config.get("model_path")
    if not isinstance(model_path, str) or not model_path:
        raise ProtocolError(
            "LOCAL_MODEL_NOT_AVAILABLE", "Mô hình giọng Việt cục bộ chưa được cài đặt."
        )
    from workers.tts.providers.local_melo import LocalMeloTtsAdapter

    directory = Path.cwd() / "audio" / "tts"
    provider = LocalMeloTtsAdapter(model_path, directory)
    items = cast(list[JsonObject], payload["items"])
    artifacts: list[JsonObject] = []
    emit_event(event(rid, "progress", progress=0, message="Đang khởi tạo giọng Việt"))
    for index, item in enumerate(items):
        audio = provider.synthesize(
            cast(str, item["text"]),
            cast(str, item["voice_id"]),
            float(cast(float, item["speed"])),
        )
        duration, rate, channels, bits = validate_wav(audio)
        segment_id = cast(str, item["segment_id"])
        relative = f"audio/tts/tts-{rid}-{segment_id}.wav"
        destination = Path.cwd() / Path(*relative.split("/"))
        with destination.open("xb") as output:
            output.write(audio)
            output.flush()
            os.fsync(output.fileno())
        artifacts.append(
            {
                "type": "tts",
                "relative_path": relative,
                "sha256": hashlib.sha256(audio).hexdigest(),
                "size_bytes": len(audio),
                "metadata": {
                    "duration_ms": duration,
                    "sample_rate": rate,
                    "channels": channels,
                    "bits_per_sample": bits,
                    "segment_id": segment_id,
                },
            }
        )
        emit_event(
            event(
                rid,
                "progress",
                progress=round((index + 1) * 100 / len(items)),
                message=f"Đã tạo giọng {index + 1}/{len(items)}",
            )
        )
    emit_event(
        event(
            rid,
            "completed",
            artifacts=cast(JsonValue, artifacts),
            metrics={"provider_id": provider.provider_id, "sends_data_off_device": False},
            warnings=[],
        )
    )


def handle(request: JsonObject) -> None:
    rid = cast(str, request["request_id"])
    action = request.get("action")
    if action not in {"synthesize", "synthesize_preview", "synthesize_batch"}:
        raise ProtocolError("UNSUPPORTED_ACTION", "Tác vụ TTS không được hỗ trợ.")
    data = request.get("input")
    config = request.get("config")
    if not isinstance(data, dict) or not isinstance(config, dict):
        raise ProtocolError("INVALID_TTS_REQUEST", "Yêu cầu TTS không hợp lệ.")
    if action == "synthesize_batch":
        handle_local_batch(request, data, config)
        return
    payload = data.get("tts")
    if not isinstance(payload, dict) or list(VALIDATOR.iter_errors(payload)):
        raise ProtocolError("INVALID_TTS_REQUEST", "Yêu cầu TTS không hợp lệ.")
    provider = resolve(config)
    attempts = config.get("max_attempts", 2)
    if not isinstance(attempts, int) or isinstance(attempts, bool):
        raise ProtocolError("INVALID_RETRY_POLICY", "Chính sách thử lại không hợp lệ.")
    emit_event(event(rid, "progress", progress=0, message="TTS started"))
    audio = synthesize_with_retry(
        provider,
        cast(str, payload["text"]),
        cast(str, payload["voice_id"]),
        float(cast(float, payload["speed"])),
        attempts,
    )
    duration, rate, channels, bits = validate_wav(audio)
    directory = "previews" if action == "synthesize_preview" else "audio/tts"
    relative = f"{directory}/tts-{rid}.wav"
    destination = Path.cwd() / Path(*relative.split("/"))
    parent = destination.parent.resolve(strict=True)
    root = Path.cwd().resolve(strict=True)
    if not parent.is_relative_to(root) or parent.is_symlink():
        raise ProtocolError("UNSAFE_PATH", "Đường dẫn TTS không hợp lệ.")
    with destination.open("xb") as output:
        output.write(audio)
        output.flush()
        os.fsync(output.fileno())
    artifact: JsonObject = {
        "type": "preview" if action == "synthesize_preview" else "tts",
        "relative_path": relative,
        "sha256": hashlib.sha256(audio).hexdigest(),
        "size_bytes": len(audio),
        "metadata": {
            "duration_ms": duration,
            "sample_rate": rate,
            "channels": channels,
            "bits_per_sample": bits,
            "segment_id": payload["segment_id"],
        },
    }
    emit_event(event(rid, "progress", progress=100, message="TTS completed"))
    emit_event(
        event(
            rid,
            "completed",
            artifacts=[artifact],
            metrics={
                "provider_id": provider.provider_id,
                "sends_data_off_device": provider.sends_data_off_device,
            },
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
                error_code="TTS_INTERNAL_ERROR",
                safe_message="Bộ máy TTS gặp lỗi nội bộ.",
            )
        )
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
