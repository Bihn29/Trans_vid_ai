from __future__ import annotations

import json

import pytest

from workers.common.protocol import ProtocolError
from workers.translation.contract import (
    GlossaryTerm,
    TemporaryProviderError,
    TranslationContext,
    TranslationSegment,
    build_prompts,
    parse_strict_result,
    translate_with_retry,
)

ID_A = "11111111-1111-4111-8111-111111111111"
ID_B = "22222222-2222-4222-8222-222222222222"


def context() -> TranslationContext:
    return TranslationContext(
        source_language="zh",
        target_language="vi",
        segments=(
            TranslationSegment(ID_A, "Alice 来了"),
            TranslationSegment(ID_B, "你好"),
        ),
        context_before=(TranslationSegment("33333333-3333-4333-8333-333333333333", "此前"),),
        context_after=(TranslationSegment("44444444-4444-4444-8444-444444444444", "后来"),),
        glossary=(GlossaryTerm("你好", "xin chào"),),
        locked_names=("Alice",),
    )


def valid_result() -> str:
    return json.dumps(
        {
            "schema_version": 1,
            "translations": [
                {"id": ID_A, "text": "Alice đã đến"},
                {"id": ID_B, "text": "Xin chào"},
            ],
        }
    )


@pytest.mark.parametrize(
    "raw",
    [
        lambda: f"Here is JSON: {valid_result()}",
        lambda: json.dumps({"schema_version": 1, "translations": [{"id": ID_A, "text": "Alice"}]}),
        lambda: json.dumps(
            {
                "schema_version": 1,
                "translations": [
                    {"id": ID_A, "text": "Alice"},
                    {"id": ID_A, "text": "Alice again"},
                ],
            }
        ),
        lambda: json.dumps(
            {
                "schema_version": 1,
                "translations": [
                    {"id": ID_A, "text": "Alice"},
                    {"id": ID_B, "text": "   "},
                ],
            }
        ),
    ],
)
def test_strict_parser_rejects_prose_missing_duplicate_and_empty_ids(raw: object) -> None:
    with pytest.raises(ProtocolError):
        parse_strict_result(raw(), context())  # type: ignore[operator]


def test_locked_name_must_be_preserved_exactly() -> None:
    payload = json.loads(valid_result())
    payload["translations"][0]["text"] = "Cô ấy đã đến"
    raw = json.dumps(payload)
    with pytest.raises(ProtocolError, match="Tên riêng"):
        parse_strict_result(raw, context())


def test_prompt_contains_context_glossary_and_locked_names() -> None:
    system, user = build_prompts(context())
    assert "Return ONLY" in system
    assert "context_before" in user
    assert "xin chào" in user
    assert "Alice" in user


class _RetryProvider:
    provider_id = "test-only"
    sends_data_off_device = False

    def __init__(self, failures: int) -> None:
        self.failures = failures
        self.calls = 0

    def translate(self, _system: str, _user: str) -> str:
        self.calls += 1
        if self.calls <= self.failures:
            raise TemporaryProviderError
        return valid_result()


def test_retry_is_bounded_and_recovers_transient_failure() -> None:
    recovers = _RetryProvider(1)
    assert translate_with_retry(recovers, context(), 2)["schema_version"] == 1
    assert recovers.calls == 2

    fails = _RetryProvider(3)
    with pytest.raises(ProtocolError):
        translate_with_retry(fails, context(), 2)
    assert fails.calls == 2
