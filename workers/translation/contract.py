"""Translation contracts, prompt construction, and strict response validation."""

from __future__ import annotations

import json
from dataclasses import dataclass
from pathlib import Path
from typing import Protocol, cast

from jsonschema import Draft202012Validator, FormatChecker

from workers.common.protocol import JsonObject, JsonValue, ProtocolError

_ROOT = Path(__file__).resolve().parents[2]
with (_ROOT / "schemas" / "translation-result.schema.json").open(encoding="utf-8") as _file:
    _RESULT_SCHEMA = cast(JsonObject, json.load(_file))
_RESULT_VALIDATOR = Draft202012Validator(_RESULT_SCHEMA, format_checker=FormatChecker())


@dataclass(frozen=True)
class TranslationSegment:
    id: str
    source_text: str


@dataclass(frozen=True)
class GlossaryTerm:
    source_text: str
    target_text: str
    case_sensitive: bool = False


@dataclass(frozen=True)
class TranslationContext:
    source_language: str
    target_language: str
    segments: tuple[TranslationSegment, ...]
    context_before: tuple[TranslationSegment, ...] = ()
    context_after: tuple[TranslationSegment, ...] = ()
    glossary: tuple[GlossaryTerm, ...] = ()
    locked_names: tuple[str, ...] = ()


class TranslationProvider(Protocol):
    """Provider adapter returning the provider's untrusted text response."""

    provider_id: str
    sends_data_off_device: bool

    def translate(self, system_prompt: str, user_prompt: str) -> str: ...


class TemporaryProviderError(Exception):
    """A provider failure that may be retried within a bounded budget."""


def build_prompts(context: TranslationContext) -> tuple[str, str]:
    """Build a deterministic prompt carrying context, glossary, and locked names."""
    if not context.segments or len(context.segments) > 20:
        raise ProtocolError("INVALID_TRANSLATION_BLOCK", "Khối dịch không hợp lệ.")
    ids = [segment.id for segment in context.segments]
    if len(set(ids)) != len(ids) or any(
        not segment.source_text.strip() for segment in context.segments
    ):
        raise ProtocolError("INVALID_TRANSLATION_BLOCK", "Khối dịch không hợp lệ.")

    system = (
        f"Translate {context.source_language} to {context.target_language}. "
        "Return ONLY one JSON object matching schema_version=1 and translations=[{id,text}]. "
        "Return every requested id exactly once; never add, remove, or rename ids. "
        "Preserve every locked proper name exactly when it occurs in a source segment."
    )
    payload: JsonObject = {
        "segments": [_segment_json(value) for value in context.segments],
        "context_before": [_segment_json(value) for value in context.context_before],
        "context_after": [_segment_json(value) for value in context.context_after],
        "glossary": [
            {
                "source_text": value.source_text,
                "target_text": value.target_text,
                "case_sensitive": value.case_sensitive,
            }
            for value in context.glossary
        ],
        "locked_names": list(context.locked_names),
    }
    return system, json.dumps(payload, ensure_ascii=False, separators=(",", ":"))


def parse_strict_result(raw: str, context: TranslationContext) -> JsonObject:
    """Reject prose, schema drift, missing/duplicate/empty IDs, and changed locked names."""
    try:
        value = cast(JsonValue, json.loads(raw))
    except json.JSONDecodeError as error:
        raise ProtocolError(
            "INVALID_TRANSLATION_OUTPUT", "Kết quả dịch không đúng cấu trúc."
        ) from error
    if not isinstance(value, dict):
        raise ProtocolError("INVALID_TRANSLATION_OUTPUT", "Kết quả dịch không đúng cấu trúc.")
    errors = sorted(_RESULT_VALIDATOR.iter_errors(value), key=lambda item: list(item.absolute_path))
    if errors:
        raise ProtocolError("INVALID_TRANSLATION_OUTPUT", "Kết quả dịch không đúng cấu trúc.")

    translations = cast(list[JsonObject], value["translations"])
    expected = {segment.id: segment for segment in context.segments}
    seen: set[str] = set()
    for item in translations:
        item_id = cast(str, item["id"])
        text = cast(str, item["text"])
        if item_id not in expected or item_id in seen or not text.strip():
            raise ProtocolError("INVALID_TRANSLATION_IDS", "ID trong kết quả dịch không hợp lệ.")
        seen.add(item_id)
        source = expected[item_id].source_text
        for locked_name in context.locked_names:
            if locked_name in source and locked_name not in text:
                raise ProtocolError("LOCKED_NAME_CHANGED", "Tên riêng bị khóa đã bị thay đổi.")
    if seen != set(expected):
        raise ProtocolError("INVALID_TRANSLATION_IDS", "Kết quả dịch thiếu ID bắt buộc.")
    return value


def translate_with_retry(
    provider: TranslationProvider,
    context: TranslationContext,
    max_attempts: int,
) -> JsonObject:
    if max_attempts < 1 or max_attempts > 3:
        raise ProtocolError("INVALID_RETRY_POLICY", "Chính sách thử lại không hợp lệ.")
    system, user = build_prompts(context)
    last_error: Exception | None = None
    for _attempt in range(max_attempts):
        try:
            return parse_strict_result(provider.translate(system, user), context)
        except (TemporaryProviderError, ProtocolError) as error:
            last_error = error
    if isinstance(last_error, ProtocolError):
        raise last_error
    raise ProtocolError("TRANSLATION_PROVIDER_FAILED", "Nhà cung cấp dịch tạm thời không khả dụng.")


def _segment_json(segment: TranslationSegment) -> JsonObject:
    return {"id": segment.id, "source_text": segment.source_text}
