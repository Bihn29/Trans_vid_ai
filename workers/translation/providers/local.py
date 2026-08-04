"""Offline OPUS-MT adapter backed by CTranslate2 and SentencePiece."""

from __future__ import annotations

import json
from pathlib import Path
from typing import Any, Protocol, cast

from workers.common.protocol import JsonObject, JsonValue, ProtocolError

_MODEL_FILES = ("model.bin", "config.json", "shared_vocabulary.json", "source.spm", "target.spm")


class LocalOpusMtAdapter:
    provider_id = "local"
    sends_data_off_device = False

    def __init__(self, model_path: str) -> None:
        root = Path(model_path)
        if (
            not root.is_absolute()
            or not root.is_dir()
            or root.is_symlink()
            or any(
                not (root / name).is_file() or (root / name).is_symlink()
                for name in _MODEL_FILES
            )
        ):
            raise ProtocolError(
                "LOCAL_MODEL_NOT_AVAILABLE",
                "Mô hình dịch Trung-Việt cục bộ chưa được cài đặt.",
            )
        self._root = root.resolve()
        self._translator: Any = None
        self._source_processor: Any = None
        self._target_processor: Any = None

    def _load(self) -> tuple[Any, Any, Any]:
        if self._translator is not None:
            return self._translator, self._source_processor, self._target_processor
        try:
            import ctranslate2  # type: ignore[import-untyped]
            import sentencepiece as spm  # type: ignore[import-untyped]

            self._translator = ctranslate2.Translator(
                str(self._root), device="cpu", compute_type="int8"
            )
            self._source_processor = spm.SentencePieceProcessor(
                model_file=str(self._root / "source.spm")
            )
            self._target_processor = spm.SentencePieceProcessor(
                model_file=str(self._root / "target.spm")
            )
        except Exception as error:
            raise ProtocolError(
                "LOCAL_MODEL_LOAD_FAILED",
                "Không thể nạp mô hình dịch Trung-Việt cục bộ.",
            ) from error
        return self._translator, self._source_processor, self._target_processor

    def translate(self, _system_prompt: str, user_prompt: str) -> str:
        try:
            value = cast(JsonValue, json.loads(user_prompt))
        except json.JSONDecodeError as error:
            raise ProtocolError("INVALID_TRANSLATION_BLOCK", "Khối dịch không hợp lệ.") from error
        if not isinstance(value, dict) or not isinstance(value.get("segments"), list):
            raise ProtocolError("INVALID_TRANSLATION_BLOCK", "Khối dịch không hợp lệ.")
        segments = cast(list[JsonObject], value["segments"])
        if not segments:
            raise ProtocolError("INVALID_TRANSLATION_BLOCK", "Khối dịch không hợp lệ.")

        ids: list[str] = []
        source_texts: list[str] = []
        for segment in segments:
            segment_id = segment.get("id")
            source_text = segment.get("source_text")
            if (
                not isinstance(segment_id, str)
                or not isinstance(source_text, str)
                or not source_text
            ):
                raise ProtocolError("INVALID_TRANSLATION_BLOCK", "Khối dịch không hợp lệ.")
            ids.append(segment_id)
            source_texts.append(source_text)

        translator, source_processor, target_processor = self._load()
        try:
            source_tokens = [source_processor.encode(text, out_type=str) for text in source_texts]
            results = translator.translate_batch(
                source_tokens,
                beam_size=4,
                max_decoding_length=256,
            )
            translated = [
                target_processor.decode(result.hypotheses[0]).strip() for result in results
            ]
        except Exception as error:
            raise ProtocolError(
                "LOCAL_TRANSLATION_FAILED",
                "Mô hình cục bộ không thể dịch khối nội dung này.",
            ) from error
        if len(translated) != len(ids) or any(not text for text in translated):
            raise ProtocolError("INVALID_TRANSLATION_OUTPUT", "Kết quả dịch không đúng cấu trúc.")

        result: JsonObject = {
            "schema_version": 1,
            "translations": [
                {"id": segment_id, "text": text}
                for segment_id, text in zip(ids, translated, strict=True)
            ],
        }
        return json.dumps(result, ensure_ascii=False, separators=(",", ":"))


class LocalTranslationTransport(Protocol):
    def translate_locally(self, system_prompt: str, user_prompt: str) -> str: ...


class LocalTranslationAdapter:
    """Provider-neutral adapter retained for injected local runtimes."""

    provider_id = "local"
    sends_data_off_device = False

    def __init__(self, transport: LocalTranslationTransport) -> None:
        self._transport = transport

    def translate(self, system_prompt: str, user_prompt: str) -> str:
        return self._transport.translate_locally(system_prompt, user_prompt)
