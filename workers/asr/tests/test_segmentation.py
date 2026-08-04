"""Tests for post-ASR segmentation normalization and QC."""

from __future__ import annotations

from workers.asr.contract import AsrSegment
from workers.asr.segmentation import check_quality, normalize_segments


class TestNormalizeSegments:
    def test_empty_input_returns_empty(self) -> None:
        assert normalize_segments([]) == []

    def test_preserves_valid_segments(self) -> None:
        segments = [
            AsrSegment(start_ms=0, end_ms=3000, text="hello", confidence=0.9),
            AsrSegment(start_ms=3000, end_ms=6000, text="world", confidence=0.85),
        ]
        result = normalize_segments(segments)
        assert len(result) == 2

    def test_merges_ultra_short_segments(self) -> None:
        segments = [
            AsrSegment(start_ms=0, end_ms=3000, text="hello", confidence=0.9),
            AsrSegment(start_ms=3000, end_ms=3100, text="x", confidence=0.5),
        ]
        result = normalize_segments(segments)
        assert len(result) == 1
        assert "x" in result[0].text

    def test_splits_ultra_long_segments(self) -> None:
        segments = [
            AsrSegment(
                start_ms=0, end_ms=30000, text="word " * 20, confidence=0.9
            ),
        ]
        result = normalize_segments(segments)
        assert len(result) >= 2
        for seg in result:
            assert seg.end_ms - seg.start_ms <= 15000

    def test_sorts_by_start_ms(self) -> None:
        segments = [
            AsrSegment(start_ms=5000, end_ms=8000, text="second", confidence=0.9),
            AsrSegment(start_ms=0, end_ms=3000, text="first", confidence=0.9),
        ]
        result = normalize_segments(segments)
        assert result[0].start_ms < result[1].start_ms


class TestCheckQuality:
    def test_detects_overlap(self) -> None:
        segments = [
            AsrSegment(start_ms=0, end_ms=3000, text="first", confidence=0.9),
            AsrSegment(start_ms=2500, end_ms=5000, text="second", confidence=0.9),
        ]
        warnings = check_quality(segments)
        assert any(w.kind == "overlap" for w in warnings)

    def test_detects_empty_text(self) -> None:
        segments = [
            AsrSegment(start_ms=0, end_ms=1000, text="  ", confidence=0.9),
        ]
        warnings = check_quality(segments)
        assert any(w.kind == "empty_text" for w in warnings)

    def test_detects_long_segment(self) -> None:
        segments = [
            AsrSegment(start_ms=0, end_ms=20000, text="long", confidence=0.9),
        ]
        warnings = check_quality(segments)
        assert any(w.kind == "long_segment" for w in warnings)

    def test_detects_low_confidence(self) -> None:
        segments = [
            AsrSegment(start_ms=0, end_ms=1000, text="uncertain", confidence=0.1),
        ]
        warnings = check_quality(segments)
        assert any(w.kind == "low_confidence" for w in warnings)

    def test_detects_silence_gap(self) -> None:
        segments = [
            AsrSegment(start_ms=0, end_ms=1000, text="first", confidence=0.9),
            AsrSegment(start_ms=7000, end_ms=8000, text="second", confidence=0.9),
        ]
        warnings = check_quality(segments)
        assert any(w.kind == "silence" for w in warnings)

    def test_detects_repetition(self) -> None:
        segments = [
            AsrSegment(start_ms=0, end_ms=1000, text="same text", confidence=0.9),
            AsrSegment(start_ms=1000, end_ms=2000, text="same text", confidence=0.9),
        ]
        warnings = check_quality(segments)
        assert any(w.kind == "repetition" for w in warnings)

    def test_clean_transcript_has_no_warnings(self) -> None:
        segments = [
            AsrSegment(start_ms=0, end_ms=3000, text="first sentence", confidence=0.9),
            AsrSegment(start_ms=3000, end_ms=6000, text="second sentence", confidence=0.85),
        ]
        warnings = check_quality(segments)
        assert len(warnings) == 0
