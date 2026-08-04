"""faster-whisper adapter implementing the normalized AsrProvider contract.

faster-whisper is imported only at runtime to avoid mandatory model downloads
during testing. This adapter normalizes output into ``AsrSegment`` instances
with validated invariants.
"""

from __future__ import annotations

from pathlib import Path
from typing import Any

from workers.asr.contract import AsrSegment
from workers.common.protocol import ProtocolError


class FasterWhisperProvider:
    """ASR provider backed by faster-whisper."""

    def __init__(self, model_path: str) -> None:
        path = Path(model_path)
        if not path.is_absolute() or not path.is_dir() or path.is_symlink():
            raise ProtocolError(
                "MODEL_NOT_AVAILABLE",
                "Mô hình faster-whisper chưa được cài đặt trong thư mục đã xác minh.",
            )
        self._model_path = str(path.resolve())
        self._model: Any = None

    def _load_model(self) -> Any:
        if self._model is not None:
            return self._model
        try:
            from faster_whisper import WhisperModel  # type: ignore[import-untyped]
        except ImportError as error:
            raise ProtocolError(
                "MODEL_NOT_AVAILABLE",
                "faster-whisper chưa được cài đặt. Vui lòng cài đặt faster-whisper.",
            ) from error
        self._model = WhisperModel(
            self._model_path,
            device="cpu",
            compute_type="int8",
            local_files_only=True,
        )
        return self._model

    def transcribe(self, audio_path: str, language: str) -> list[AsrSegment]:
        """Transcribe audio using faster-whisper and return normalized segments."""
        segments: list[AsrSegment] = []
        try:
            model = self._load_model()
        except Exception as error:
            raise ProtocolError(
                "MODEL_LOAD_FAILED",
                "faster-whisper không thể nạp mô hình nhận dạng giọng nói.",
            ) from error
        try:
            whisper_segments, _info = model.transcribe(
                audio_path,
                language=language,
                beam_size=5,
                vad_filter=True,
            )
            for ws in whisper_segments:
                start_ms = int(ws.start * 1000)
                end_ms = int(ws.end * 1000)
                text = ws.text.strip() if isinstance(ws.text, str) else ""
                if not text or end_ms <= start_ms:
                    continue
                confidence = 0.0
                if hasattr(ws, "avg_logprob"):
                    import math

                    confidence = max(0.0, min(1.0, math.exp(ws.avg_logprob)))
                segments.append(
                    AsrSegment(
                        start_ms=start_ms,
                        end_ms=end_ms,
                        text=text,
                        confidence=confidence,
                        speaker_label=None,
                    )
                )
        except Exception as error:
            raise ProtocolError(
                "TRANSCRIPTION_FAILED",
                "faster-whisper không thể nhận dạng giọng nói.",
            ) from error

        return segments
