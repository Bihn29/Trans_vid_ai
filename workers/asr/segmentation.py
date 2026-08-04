"""Post-ASR segmentation normalization and quality checking."""

from __future__ import annotations

from collections.abc import Sequence
from dataclasses import dataclass

from workers.asr.contract import AsrSegment

MAX_SEGMENT_DURATION_MS = 15_000
MIN_SEGMENT_DURATION_MS = 200
LOW_CONFIDENCE_THRESHOLD = 0.3
SILENCE_THRESHOLD_MS = 5_000


@dataclass(frozen=True, slots=True)
class SegmentWarning:
    """A quality warning about a transcript segment."""

    kind: str
    segment_index: int
    message: str
    detail: dict[str, int | float | str] | None = None


def normalize_segments(segments: list[AsrSegment]) -> list[AsrSegment]:
    """Merge ultra-short segments and split ultra-long ones.

    Returns a new list of segments with corrected boundaries, sorted by
    ``start_ms``. Original segments are not mutated.
    """
    if not segments:
        return []

    sorted_segments = sorted(segments, key=lambda s: s.start_ms)
    result: list[AsrSegment] = []

    for segment in sorted_segments:
        duration = segment.end_ms - segment.start_ms

        if duration < MIN_SEGMENT_DURATION_MS and result:
            previous = result[-1]
            merged = AsrSegment(
                start_ms=previous.start_ms,
                end_ms=max(previous.end_ms, segment.end_ms),
                text=f"{previous.text} {segment.text}".strip(),
                confidence=min(previous.confidence, segment.confidence),
                speaker_label=previous.speaker_label,
            )
            result[-1] = merged
            continue

        if duration > MAX_SEGMENT_DURATION_MS:
            parts = _split_long_segment(segment)
            result.extend(parts)
            continue

        result.append(segment)

    return result


def _split_long_segment(segment: AsrSegment) -> list[AsrSegment]:
    """Split a segment that exceeds the maximum duration into equal parts."""
    duration = segment.end_ms - segment.start_ms
    num_parts = (duration + MAX_SEGMENT_DURATION_MS - 1) // MAX_SEGMENT_DURATION_MS
    part_duration = duration // num_parts

    text = segment.text.strip()
    words = text.split()
    words_per_part = max(1, len(words) // num_parts)

    parts: list[AsrSegment] = []
    for i in range(num_parts):
        part_start = segment.start_ms + i * part_duration
        if i < num_parts - 1:
            part_end = segment.start_ms + (i + 1) * part_duration
        else:
            part_end = segment.end_ms
        word_start = i * words_per_part
        word_end = (i + 1) * words_per_part if i < num_parts - 1 else len(words)
        part_text = " ".join(words[word_start:word_end]) or text

        parts.append(
            AsrSegment(
                start_ms=part_start,
                end_ms=part_end,
                text=part_text,
                confidence=segment.confidence,
                speaker_label=segment.speaker_label,
            )
        )

    return parts


def check_quality(segments: Sequence[AsrSegment]) -> list[SegmentWarning]:
    """Check transcript segments for common quality issues.

    Returns warnings for: overlap, empty text, long segments, repetition,
    silence gaps, and low confidence. Warnings do not prevent processing
    but are surfaced at the transcript review checkpoint.
    """
    warnings: list[SegmentWarning] = []

    for i, segment in enumerate(segments):
        if not segment.text.strip():
            warnings.append(
                SegmentWarning(
                    kind="empty_text",
                    segment_index=i,
                    message="Đoạn không có nội dung văn bản.",
                )
            )

        duration = segment.end_ms - segment.start_ms
        if duration > MAX_SEGMENT_DURATION_MS:
            warnings.append(
                SegmentWarning(
                    kind="long_segment",
                    segment_index=i,
                    message=f"Đoạn quá dài ({duration}ms > {MAX_SEGMENT_DURATION_MS}ms).",
                    detail={"duration_ms": duration},
                )
            )

        if segment.confidence < LOW_CONFIDENCE_THRESHOLD:
            warnings.append(
                SegmentWarning(
                    kind="low_confidence",
                    segment_index=i,
                    message=f"Độ tin cậy thấp ({segment.confidence:.0%}).",
                    detail={"confidence": segment.confidence},
                )
            )

    for i in range(1, len(segments)):
        current = segments[i]
        previous = segments[i - 1]

        if current.start_ms < previous.end_ms:
            warnings.append(
                SegmentWarning(
                    kind="overlap",
                    segment_index=i,
                    message="Đoạn này chồng lên đoạn trước.",
                    detail={
                        "overlap_ms": previous.end_ms - current.start_ms,
                    },
                )
            )

        gap = current.start_ms - previous.end_ms
        if gap > SILENCE_THRESHOLD_MS:
            warnings.append(
                SegmentWarning(
                    kind="silence",
                    segment_index=i,
                    message=f"Khoảng lặng dài ({gap}ms) trước đoạn này.",
                    detail={"gap_ms": gap},
                )
            )

        if (
            previous.text.strip()
            and current.text.strip()
            and previous.text.strip() == current.text.strip()
        ):
            warnings.append(
                SegmentWarning(
                    kind="repetition",
                    segment_index=i,
                    message="Đoạn này lặp lại nội dung đoạn trước.",
                )
            )

    return warnings
