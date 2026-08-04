from __future__ import annotations

import json
import sys
from pathlib import Path
from types import ModuleType, SimpleNamespace

import pytest

from workers.translation.providers.local import LocalOpusMtAdapter


def _model_root(tmp_path: Path) -> Path:
    for name in ("model.bin", "config.json", "shared_vocabulary.json", "source.spm", "target.spm"):
        (tmp_path / name).write_bytes(b"test")
    return tmp_path


def test_local_adapter_translates_every_segment(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    ctranslate2 = ModuleType("ctranslate2")
    sentencepiece = ModuleType("sentencepiece")

    class FakeTranslator:
        def __init__(self, _root: str, **kwargs: object) -> None:
            assert kwargs == {"device": "cpu", "compute_type": "int8"}

        def translate_batch(self, batches: list[list[str]], **_kwargs: object) -> list[object]:
            return [SimpleNamespace(hypotheses=[[f"vi:{batch[0]}"]]) for batch in batches]

    class FakeProcessor:
        def __init__(self, **_kwargs: object) -> None:
            pass

        def encode(self, text: str, **_kwargs: object) -> list[str]:
            return [text]

        def decode(self, tokens: list[str]) -> str:
            return tokens[0]

    ctranslate2.Translator = FakeTranslator  # type: ignore[attr-defined]
    sentencepiece.SentencePieceProcessor = FakeProcessor  # type: ignore[attr-defined]
    monkeypatch.setitem(sys.modules, "ctranslate2", ctranslate2)
    monkeypatch.setitem(sys.modules, "sentencepiece", sentencepiece)

    adapter = LocalOpusMtAdapter(str(_model_root(tmp_path)))
    response = json.loads(
        adapter.translate(
            "unused",
            json.dumps(
                {
                    "segments": [
                        {"id": "11111111-1111-4111-8111-111111111111", "source_text": "你好"},
                        {"id": "22222222-2222-4222-8222-222222222222", "source_text": "再见"},
                    ]
                }
            ),
        )
    )

    assert [item["text"] for item in response["translations"]] == ["vi:你好", "vi:再见"]
