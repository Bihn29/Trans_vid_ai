"""Versioned JSONL entry point for one persistent translation block."""

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
    is_safe_relative_path,
    read_request,
)
from workers.translation.contract import (
    GlossaryTerm,
    TranslationContext,
    TranslationProvider,
    TranslationSegment,
    translate_with_retry,
)
from workers.translation.providers.local import LocalOpusMtAdapter
from workers.translation.providers.openai_compatible import OpenAiCompatibleAdapter

_FALLBACK_REQUEST_ID = "00000000-0000-4000-8000-000000000000"
_ROOT = Path(__file__).resolve().parents[2]
with (_ROOT / "schemas" / "translation-request.schema.json").open(encoding="utf-8") as _file:
    _REQUEST_SCHEMA = cast(JsonObject, json.load(_file))
_BLOCK_VALIDATOR = Draft202012Validator(_REQUEST_SCHEMA, format_checker=FormatChecker())


def _event(request_id: str, event: str, **values: JsonValue) -> JsonObject:
    return {
        "protocol_version": PROTOCOL_VERSION,
        "request_id": request_id,
        "event": event,
        **values,
    }


def _request_id(request: JsonObject | None) -> str:
    value = request.get("request_id") if request is not None else None
    return value if isinstance(value, str) else _FALLBACK_REQUEST_ID


def _resolve_provider(config: JsonObject) -> TranslationProvider:
    provider_id = config.get("provider_id")
    if provider_id == "openai-compatible":
        if config.get("cloud_consent") is not True:
            raise ProtocolError(
                "CLOUD_CONSENT_REQUIRED", "Cần xác nhận trước khi gửi văn bản ra ngoài thiết bị."
            )
        endpoint = config.get("endpoint")
        model = config.get("model")
        api_key = config.get("api_key")
        if not all(isinstance(value, str) for value in (endpoint, model, api_key)):
            raise ProtocolError(
                "INVALID_PROVIDER_CONFIG", "Cấu hình nhà cung cấp dịch không hợp lệ."
            )
        return OpenAiCompatibleAdapter(cast(str, endpoint), cast(str, model), cast(str, api_key))
    if provider_id == "local":
        model_path = config.get("model_path")
        if not isinstance(model_path, str) or not model_path:
            raise ProtocolError(
                "LOCAL_MODEL_NOT_AVAILABLE", "Mô hình dịch Trung-Việt cục bộ chưa được cài đặt."
            )
        return LocalOpusMtAdapter(model_path)
    raise ProtocolError("UNSUPPORTED_PROVIDER", "Nhà cung cấp dịch không được hỗ trợ.")


def _parse_context(block: JsonObject) -> TranslationContext:
    errors = sorted(_BLOCK_VALIDATOR.iter_errors(block), key=lambda item: list(item.absolute_path))
    if errors:
        raise ProtocolError("INVALID_TRANSLATION_BLOCK", "Khối dịch không hợp lệ.")

    def segments(key: str) -> tuple[TranslationSegment, ...]:
        values = cast(list[JsonObject], block[key])
        return tuple(
            TranslationSegment(
                id=cast(str, value["id"]), source_text=cast(str, value["source_text"])
            )
            for value in values
        )

    glossary_values = cast(list[JsonObject], block["glossary"])
    return TranslationContext(
        source_language=cast(str, block["source_language"]),
        target_language=cast(str, block["target_language"]),
        segments=segments("segments"),
        context_before=segments("context_before"),
        context_after=segments("context_after"),
        glossary=tuple(
            GlossaryTerm(
                source_text=cast(str, item["source_text"]),
                target_text=cast(str, item["target_text"]),
                case_sensitive=cast(bool, item["case_sensitive"]),
            )
            for item in glossary_values
        ),
        locked_names=tuple(cast(list[str], block["locked_names"])),
    )


def _write_result(request_id: str, output_directory: str, result: JsonObject) -> JsonObject:
    relative_path = f"{output_directory}/translation-{request_id}.json"
    if not is_safe_relative_path(relative_path):
        raise ProtocolError("UNSAFE_PATH", "Đường dẫn kết quả dịch không hợp lệ.")
    project_root = Path.cwd().resolve(strict=True)
    parent = (project_root / Path(*output_directory.split("/"))).resolve(strict=True)
    if not parent.is_dir() or not parent.is_relative_to(project_root) or parent.is_symlink():
        raise ProtocolError("UNSAFE_PATH", "Thư mục kết quả dịch không hợp lệ.")
    destination = parent / f"translation-{request_id}.json"
    content = json.dumps(result, ensure_ascii=False, separators=(",", ":")).encode("utf-8")
    try:
        with destination.open("xb") as output:
            output.write(content)
            output.flush()
            os.fsync(output.fileno())
    except FileExistsError as error:
        raise ProtocolError("OUTPUT_ALREADY_EXISTS", "Kết quả dịch đã tồn tại.") from error
    return {
        "type": "translation_block",
        "relative_path": relative_path,
        "sha256": hashlib.sha256(content).hexdigest(),
        "size_bytes": len(content),
        "metadata": {"translation_count": len(cast(list[JsonValue], result["translations"]))},
    }


def handle(request: JsonObject) -> None:
    request_id = cast(str, request["request_id"])
    if request.get("action") != "translate_block":
        emit_event(
            _event(
                request_id,
                "failed",
                error_code="UNSUPPORTED_ACTION",
                safe_message="Tác vụ dịch không được hỗ trợ.",
            )
        )
        return
    input_value = request.get("input")
    config_value = request.get("config")
    if not isinstance(input_value, dict) or not isinstance(config_value, dict):
        raise ProtocolError("INVALID_TRANSLATION_BLOCK", "Khối dịch không hợp lệ.")
    block = input_value.get("block")
    if not isinstance(block, dict):
        raise ProtocolError("INVALID_TRANSLATION_BLOCK", "Khối dịch không hợp lệ.")
    context = _parse_context(block)
    max_attempts = config_value.get("max_attempts", 2)
    if not isinstance(max_attempts, int) or isinstance(max_attempts, bool):
        raise ProtocolError("INVALID_RETRY_POLICY", "Chính sách thử lại không hợp lệ.")

    emit_event(_event(request_id, "progress", progress=0, message="Translation block started"))
    provider = _resolve_provider(config_value)
    result = translate_with_retry(provider, context, max_attempts)
    artifact = _write_result(request_id, cast(str, request["output_directory"]), result)
    emit_event(_event(request_id, "progress", progress=100, message="Translation block completed"))
    emit_event(
        _event(
            request_id,
            "completed",
            artifacts=[artifact],
            metrics={
                "worker": "translation",
                "provider_id": provider.provider_id,
                "sends_data_off_device": provider.sends_data_off_device,
                "translation_count": len(context.segments),
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
            _event(
                _request_id(request),
                "failed",
                error_code=error.error_code,
                safe_message=error.safe_message,
            )
        )
        return 0
    except (OSError, ValueError, TypeError):
        emit_event(
            _event(
                _request_id(request),
                "failed",
                error_code="TRANSLATION_INTERNAL_ERROR",
                safe_message="Bộ máy dịch gặp lỗi nội bộ.",
            )
        )
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
