"""ASR worker entry point for VietDub Studio.

Receives a transcription request on stdin, dispatches to the configured
ASR provider, normalizes segments, and emits results on stdout using
the versioned JSON Lines protocol.
"""

from __future__ import annotations

import hashlib
import json
import os
import sys
from pathlib import Path
from typing import cast

if __package__ in {None, ""}:
    sys.path.insert(0, str(Path(__file__).resolve().parents[2]))

from workers.asr.contract import AsrProvider, AsrSegment, validate_segments
from workers.asr.providers.fallback_provider import FallbackAsrProvider
from workers.asr.segmentation import check_quality, normalize_segments
from workers.common.protocol import (
    PROTOCOL_VERSION,
    JsonObject,
    JsonValue,
    ProtocolError,
    emit_event,
    is_safe_relative_path,
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


def _completed(
    request_id: str,
    artifacts: list[JsonObject],
    metrics: JsonObject,
    warnings: list[str],
) -> JsonObject:
    artifacts_value: list[JsonValue] = [cast(JsonValue, a) for a in artifacts]
    warnings_value: list[JsonValue] = [cast(JsonValue, w) for w in warnings]
    return {
        "protocol_version": PROTOCOL_VERSION,
        "request_id": request_id,
        "event": "completed",
        "artifacts": artifacts_value,
        "metrics": metrics,
        "warnings": warnings_value,
    }


def _failed(request_id: str, error_code: str, safe_message: str) -> JsonObject:
    return {
        "protocol_version": PROTOCOL_VERSION,
        "request_id": request_id,
        "event": "failed",
        "error_code": error_code,
        "safe_message": safe_message,
    }


def _resolve_provider(
    model_id: str, config: JsonObject
) -> AsrProvider:
    """Resolve the ASR provider based on model_id prefix.

    Model paths are supplied by the trusted Rust model boundary and must
    already exist locally. Adapters never receive a remote model identifier.
    """
    if not model_id.startswith(("funasr:", "faster-whisper:")):
        raise ProtocolError("UNSUPPORTED_MODEL", f"Mô hình '{model_id}' không được hỗ trợ.")

    primary_model_path = config.get("primary_model_path")
    if not isinstance(primary_model_path, str) or not primary_model_path:
        raise ProtocolError(
            "MODEL_NOT_AVAILABLE",
            "Mô hình nhận dạng chính chưa được cài đặt.",
        )

    if model_id.startswith("funasr:"):
        from workers.asr.providers.faster_whisper_provider import FasterWhisperProvider
        from workers.asr.providers.funasr_provider import FunAsrProvider

        fallback_model_id = config.get("fallback_model_id")
        fallback_model_path = config.get("fallback_model_path")
        if (
            not isinstance(fallback_model_id, str)
            or not fallback_model_id.startswith("faster-whisper:")
            or not isinstance(fallback_model_path, str)
            or not fallback_model_path
        ):
            raise ProtocolError(
                "FALLBACK_MODEL_NOT_AVAILABLE",
                "Mô hình nhận dạng dự phòng chưa được cài đặt.",
            )
        return FallbackAsrProvider(
            FunAsrProvider(primary_model_path),
            FasterWhisperProvider(fallback_model_path),
        )

    if model_id.startswith("faster-whisper:"):
        from workers.asr.providers.faster_whisper_provider import FasterWhisperProvider

        return FasterWhisperProvider(primary_model_path)

    raise ProtocolError("UNSUPPORTED_MODEL", f"Mô hình '{model_id}' không được hỗ trợ.")


def _segments_to_artifact(
    request_id: str,
    segments: list[AsrSegment],
    output_directory: str,
) -> JsonObject:
    """Write normalized segments and return a verified artifact descriptor."""
    relative_path = f"{output_directory}/transcript-{request_id}.json"
    data: JsonObject = {
        "schema_version": 1,
        "segments": [
            {
                "start_ms": s.start_ms,
                "end_ms": s.end_ms,
                "text": s.text,
                "confidence": s.confidence,
                "speaker_label": s.speaker_label,
            }
            for s in segments
        ],
    }
    content = json.dumps(data, ensure_ascii=False, indent=2).encode("utf-8")
    destination = _resolve_project_path(relative_path, expect_file=False)
    try:
        with destination.open("xb") as output_file:
            output_file.write(content)
            output_file.flush()
            os.fsync(output_file.fileno())
    except FileExistsError as error:
        raise ProtocolError(
            "OUTPUT_ALREADY_EXISTS",
            "Tệp transcript đầu ra đã tồn tại.",
        ) from error

    sha256 = hashlib.sha256(content).hexdigest()

    artifact: JsonObject = {
        "type": "transcript",
        "relative_path": relative_path,
        "sha256": sha256,
        "size_bytes": len(content),
        "metadata": {"segment_count": len(segments)},
    }
    return artifact


def _resolve_project_path(relative_path: str, *, expect_file: bool) -> Path:
    """Resolve a relative path without allowing symlink or project-root escape."""
    if not is_safe_relative_path(relative_path):
        raise ProtocolError("UNSAFE_PATH", "Đường dẫn tác vụ không hợp lệ.")

    project_root = Path.cwd().resolve(strict=True)
    candidate = project_root
    parts = relative_path.split("/")
    parent_parts = parts if expect_file else parts[:-1]
    for part in parent_parts:
        candidate = candidate / part
        if candidate.is_symlink():
            raise ProtocolError("UNSAFE_PATH", "Đường dẫn tác vụ không hợp lệ.")

    if expect_file:
        resolved = (project_root / Path(*parts)).resolve(strict=True)
        if not resolved.is_file() or not resolved.is_relative_to(project_root):
            raise ProtocolError("UNSAFE_PATH", "Đường dẫn âm thanh không hợp lệ.")
        return resolved

    parent = (project_root / Path(*parts[:-1])).resolve(strict=True)
    if not parent.is_dir() or not parent.is_relative_to(project_root):
        raise ProtocolError("UNSAFE_PATH", "Thư mục đầu ra không hợp lệ.")
    return parent / parts[-1]


def _apply_region(segments: list[AsrSegment], config: JsonObject) -> list[AsrSegment]:
    start = config.get("region_start_ms")
    end = config.get("region_end_ms")
    if start is None and end is None:
        return segments
    if (
        not isinstance(start, int)
        or isinstance(start, bool)
        or not isinstance(end, int)
        or isinstance(end, bool)
        or start < 0
        or end <= start
    ):
        raise ProtocolError("INVALID_ASR_REGION", "Vùng nhận dạng được chọn không hợp lệ.")

    regional: list[AsrSegment] = []
    for segment in segments:
        segment_start = max(segment.start_ms, start)
        segment_end = min(segment.end_ms, end)
        if segment_end <= segment_start:
            continue
        regional.append(
            AsrSegment(
                start_ms=segment_start,
                end_ms=segment_end,
                text=segment.text,
                confidence=segment.confidence,
                speaker_label=segment.speaker_label,
            )
        )
    return regional


def handle(request: JsonObject) -> None:
    """Process a transcription request."""
    request_id = cast(str, request["request_id"])
    action = cast(str, request["action"])

    if action != "transcribe":
        emit_event(_failed(request_id, "UNSUPPORTED_ACTION", "Bộ máy không hỗ trợ tác vụ này."))
        return

    input_data = request.get("input", {})
    config = request.get("config", {})
    output_directory = cast(str, request["output_directory"])

    if not isinstance(input_data, dict) or not isinstance(config, dict):
        emit_event(
            _failed(request_id, "INVALID_INPUT", "Dữ liệu đầu vào không hợp lệ.")
        )
        return

    audio_path = input_data.get("audio_path", "")
    model_id = input_data.get("model_id", "")
    language = input_data.get("language", "zh")

    if not isinstance(audio_path, str) or not isinstance(model_id, str):
        emit_event(
            _failed(request_id, "INVALID_INPUT", "Đường dẫn âm thanh hoặc mô hình không hợp lệ.")
        )
        return

    try:
        resolved_audio_path = _resolve_project_path(audio_path, expect_file=True)
    except ProtocolError as error:
        emit_event(_failed(request_id, error.error_code, error.safe_message))
        return

    emit_event(_progress(request_id, 0, "ASR worker started"))

    try:
        provider = _resolve_provider(model_id, config)
    except ProtocolError as error:
        emit_event(_failed(request_id, error.error_code, error.safe_message))
        return

    emit_event(_progress(request_id, 10, "Model loaded"))

    if not isinstance(language, str) or not language:
        language = "zh"

    try:
        raw_segments = provider.transcribe(str(resolved_audio_path), language)
    except ProtocolError as error:
        emit_event(_failed(request_id, error.error_code, error.safe_message))
        return

    emit_event(_progress(request_id, 60, "Transcription complete"))

    segments = normalize_segments(raw_segments)
    try:
        segments = _apply_region(segments, config)
    except ProtocolError as error:
        emit_event(_failed(request_id, error.error_code, error.safe_message))
        return
    validation_errors = validate_segments(segments)
    if validation_errors:
        emit_event(
            _failed(
                request_id,
                "INVALID_ASR_OUTPUT",
                f"Kết quả nhận dạng không hợp lệ: {validation_errors[0]}",
            )
        )
        return

    emit_event(_progress(request_id, 80, "Segments normalized"))

    qc_warnings = check_quality(segments)
    warning_messages = [w.message for w in qc_warnings]

    try:
        artifact = _segments_to_artifact(request_id, segments, output_directory)
    except (OSError, ProtocolError) as error:
        if isinstance(error, ProtocolError):
            emit_event(_failed(request_id, error.error_code, error.safe_message))
        else:
            emit_event(
                _failed(request_id, "OUTPUT_WRITE_FAILED", "Không thể ghi transcript đầu ra.")
            )
        return

    if not is_safe_relative_path(cast(str, artifact["relative_path"])):
        emit_event(
            _failed(request_id, "INVALID_OUTPUT_PATH", "Đường dẫn đầu ra không hợp lệ.")
        )
        return

    emit_event(_progress(request_id, 100, "ASR worker completed"))
    emit_event(
        _completed(
            request_id,
            [artifact],
            {
                "worker": "asr",
                "model_id": model_id,
                "segment_count": len(segments),
                "warning_count": len(qc_warnings),
            },
            warning_messages,
        )
    )


def main() -> int:
    """Entry point."""
    request: JsonObject | None = None
    try:
        request = read_request()
        handle(request)
        return 0
    except ProtocolError as error:
        emit_event(_failed(_request_id(request), error.error_code, error.safe_message))
        return 0
    except Exception:
        emit_event(
            _failed(
                _request_id(request),
                "ASR_INTERNAL_ERROR",
                "Bộ máy nhận dạng giọng nói gặp lỗi nội bộ.",
            )
        )
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
