"""Normalized ASR provider contract.

Every ASR engine adapter must implement the ``AsrProvider`` protocol.
The contract normalizes raw engine output into ``AsrSegment`` instances
with validated invariants (positive duration, non-empty text, bounded
confidence). Business logic never depends on a specific engine import.
"""

from __future__ import annotations

from collections.abc import Sequence
from dataclasses import dataclass
from typing import Protocol


@dataclass(frozen=True, slots=True)
class AsrSegment:
    """A single recognized speech segment."""

    start_ms: int
    end_ms: int
    text: str
    confidence: float
    speaker_label: str | None = None

    def validate(self) -> list[str]:
        """Return a list of validation errors, empty when valid."""
        errors: list[str] = []
        if self.end_ms <= self.start_ms:
            errors.append(
                f"end_ms ({self.end_ms}) must be greater than start_ms ({self.start_ms})"
            )
        if not self.text.strip():
            errors.append("text must not be empty or whitespace-only")
        if not (0.0 <= self.confidence <= 1.0):
            errors.append(f"confidence ({self.confidence}) must be in [0, 1]")
        return errors


def validate_segments(segments: Sequence[AsrSegment]) -> list[str]:
    """Validate a sequence of ASR segments and return all errors."""
    errors: list[str] = []
    for i, segment in enumerate(segments):
        for error in segment.validate():
            errors.append(f"segment[{i}]: {error}")
    for i in range(1, len(segments)):
        if segments[i].start_ms < segments[i - 1].end_ms:
            errors.append(
                f"segment[{i}] overlaps segment[{i - 1}]: "
                f"start_ms={segments[i].start_ms} < prev end_ms={segments[i - 1].end_ms}"
            )
    return errors


class AsrProvider(Protocol):
    """Protocol for ASR engine adapters.

    Implementations must return segments in ascending timestamp order.
    Model imports happen only inside the implementation, never at module level.
    """

    def transcribe(self, audio_path: str, language: str) -> list[AsrSegment]:
        """Transcribe audio and return normalized segments."""
        ...
