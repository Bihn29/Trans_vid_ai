"""Tests for the ASR provider contract and deterministic stub."""

from __future__ import annotations

from workers.asr.contract import AsrSegment, validate_segments
from workers.asr.providers.fallback_provider import FallbackAsrProvider
from workers.common.protocol import ProtocolError


class TestAsrSegmentValidation:
    def test_valid_segment_has_no_errors(self) -> None:
        segment = AsrSegment(start_ms=0, end_ms=2000, text="hello", confidence=0.9)
        assert segment.validate() == []

    def test_end_before_start_is_invalid(self) -> None:
        segment = AsrSegment(start_ms=3000, end_ms=1000, text="hello", confidence=0.9)
        errors = segment.validate()
        assert any("end_ms" in e for e in errors)

    def test_empty_text_is_invalid(self) -> None:
        segment = AsrSegment(start_ms=0, end_ms=1000, text="   ", confidence=0.9)
        errors = segment.validate()
        assert any("empty" in e for e in errors)

    def test_confidence_out_of_range_is_invalid(self) -> None:
        segment = AsrSegment(start_ms=0, end_ms=1000, text="hello", confidence=1.5)
        errors = segment.validate()
        assert any("confidence" in e for e in errors)


class TestValidateSegments:
    def test_valid_sequence_has_no_errors(self) -> None:
        segments = [
            AsrSegment(start_ms=0, end_ms=2000, text="first", confidence=0.9),
            AsrSegment(start_ms=2000, end_ms=4000, text="second", confidence=0.85),
        ]
        assert validate_segments(segments) == []

    def test_overlapping_segments_are_reported(self) -> None:
        segments = [
            AsrSegment(start_ms=0, end_ms=3000, text="first", confidence=0.9),
            AsrSegment(start_ms=2000, end_ms=5000, text="second", confidence=0.85),
        ]
        errors = validate_segments(segments)
        assert any("overlap" in e.lower() for e in errors)


class _TestProvider:
    def __init__(self, segments: list[AsrSegment] | None = None, *, fail: bool = False) -> None:
        self._segments = segments or [
            AsrSegment(start_ms=0, end_ms=1000, text="test", confidence=0.9)
        ]
        self._fail = fail

    def transcribe(self, audio_path: str, language: str) -> list[AsrSegment]:
        _ = audio_path, language
        if self._fail:
            raise ProtocolError("TEST_FAILURE", "Test provider failed.")
        return list(self._segments)


class TestFallbackProvider:
    def test_returns_valid_default_segments(self) -> None:
        provider = _TestProvider()
        segments = provider.transcribe("dummy.wav", "zh")
        assert len(segments) == 1
        assert validate_segments(segments) == []

    def test_uses_primary_when_it_succeeds(self) -> None:
        primary = _TestProvider(
            [AsrSegment(start_ms=0, end_ms=1000, text="primary", confidence=0.95)]
        )
        fallback = _TestProvider(
            [AsrSegment(start_ms=0, end_ms=1000, text="fallback", confidence=0.95)]
        )
        result = FallbackAsrProvider(primary, fallback).transcribe("dummy.wav", "zh")
        assert len(result) == 1
        assert result[0].text == "primary"

    def test_falls_back_when_primary_fails(self) -> None:
        fallback = _TestProvider(
            [AsrSegment(start_ms=0, end_ms=1000, text="fallback", confidence=0.95)]
        )
        result = FallbackAsrProvider(_TestProvider(fail=True), fallback).transcribe(
            "dummy.wav", "zh"
        )
        assert result[0].text == "fallback"

    def test_reports_safe_failure_when_both_fail(self) -> None:
        provider = FallbackAsrProvider(_TestProvider(fail=True), _TestProvider(fail=True))
        try:
            provider.transcribe("dummy.wav", "zh")
        except ProtocolError as error:
            assert error.error_code == "ASR_FALLBACK_FAILED"
        else:
            raise AssertionError("both failures must be surfaced")
