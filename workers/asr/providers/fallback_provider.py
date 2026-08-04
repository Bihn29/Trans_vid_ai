"""Fallback composition for normalized ASR providers."""

from __future__ import annotations

from workers.asr.contract import AsrProvider, AsrSegment
from workers.common.protocol import ProtocolError


class FallbackAsrProvider:
    """Try the preferred provider and use the consented fallback on failure."""

    def __init__(self, primary: AsrProvider, fallback: AsrProvider) -> None:
        self._primary = primary
        self._fallback = fallback

    def transcribe(self, audio_path: str, language: str) -> list[AsrSegment]:
        try:
            return self._primary.transcribe(audio_path, language)
        except ProtocolError:
            try:
                return self._fallback.transcribe(audio_path, language)
            except ProtocolError as error:
                raise ProtocolError(
                    "ASR_FALLBACK_FAILED",
                    "Cả bộ máy nhận dạng chính và dự phòng đều không thể hoàn tất tác vụ.",
                ) from error
