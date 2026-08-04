"""FunASR adapter implementing the normalized AsrProvider contract.

FunASR is imported only at runtime to avoid mandatory model downloads
during testing. This adapter normalizes FunASR output into ``AsrSegment``
instances with validated invariants.
"""

from __future__ import annotations

from pathlib import Path
from typing import Any

from workers.asr.contract import AsrSegment
from workers.common.protocol import ProtocolError


class FunAsrProvider:
    """ASR provider backed by FunASR."""

    def __init__(self, model_path: str) -> None:
        path = Path(model_path)
        if not path.is_absolute() or not path.is_dir() or path.is_symlink():
            raise ProtocolError(
                "MODEL_NOT_AVAILABLE",
                "Mô hình FunASR chưa được cài đặt trong thư mục đã xác minh.",
            )
        self._model_path = str(path.resolve())
        self._model: Any = None

    def _load_model(self) -> Any:
        if self._model is not None:
            return self._model
        try:
            from funasr import AutoModel  # type: ignore[import-not-found]
        except ImportError as error:
            raise ProtocolError(
                "MODEL_NOT_AVAILABLE",
                "FunASR chưa được cài đặt. Vui lòng cài đặt funasr.",
            ) from error
        self._model = AutoModel(model=self._model_path)
        return self._model

    def transcribe(self, audio_path: str, language: str) -> list[AsrSegment]:
        """Transcribe audio using FunASR and return normalized segments."""
        _ = language
        model = self._load_model()
        try:
            results: list[dict[str, object]] = model.generate(
                input=audio_path,
                batch_size_s=300,
            )
        except Exception as error:
            raise ProtocolError(
                "TRANSCRIPTION_FAILED",
                "FunASR không thể nhận dạng giọng nói.",
            ) from error

        segments: list[AsrSegment] = []
        for result in results:
            sentence_list = result.get("sentence_info", [])
            if not isinstance(sentence_list, list):
                continue
            for sentence in sentence_list:
                if not isinstance(sentence, dict):
                    continue
                start = sentence.get("start", 0)
                end = sentence.get("end", 0)
                text = sentence.get("text", "")
                if not isinstance(start, int) or not isinstance(end, int):
                    continue
                if not isinstance(text, str) or not text.strip():
                    continue
                if end <= start:
                    continue
                segments.append(
                    AsrSegment(
                        start_ms=start,
                        end_ms=end,
                        text=text.strip(),
                        confidence=0.8,
                        speaker_label=None,
                    )
                )

        return segments
