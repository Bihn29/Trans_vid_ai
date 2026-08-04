from __future__ import annotations

import sys
from pathlib import Path
from types import ModuleType, SimpleNamespace
from typing import Any

import pytest

from workers.asr.providers.faster_whisper_provider import FasterWhisperProvider
from workers.common.protocol import ProtocolError


def _install_fake_module(monkeypatch: pytest.MonkeyPatch, model_type: type[Any]) -> None:
    module = ModuleType("faster_whisper")
    module.WhisperModel = model_type  # type: ignore[attr-defined]
    monkeypatch.setitem(sys.modules, "faster_whisper", module)


def test_provider_forces_cpu_int8(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    captured: dict[str, object] = {}

    class FakeModel:
        def __init__(self, model_path: str, **kwargs: object) -> None:
            captured["model_path"] = model_path
            captured.update(kwargs)

        def transcribe(self, _audio_path: str, **_kwargs: object) -> tuple[list[object], None]:
            return (
                [SimpleNamespace(start=0.0, end=1.0, text="你好", avg_logprob=-0.1)],
                None,
            )

    _install_fake_module(monkeypatch, FakeModel)
    provider = FasterWhisperProvider(str(tmp_path))

    segments = provider.transcribe("audio.wav", "zh")

    assert captured["device"] == "cpu"
    assert captured["compute_type"] == "int8"
    assert captured["local_files_only"] is True
    assert segments[0].text == "你好"


def test_provider_maps_lazy_runtime_failure(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    class FakeModel:
        def __init__(self, _model_path: str, **_kwargs: object) -> None:
            pass

        def transcribe(self, _audio_path: str, **_kwargs: object) -> tuple[object, None]:
            def broken_segments() -> object:
                raise RuntimeError("native runtime unavailable")
                yield None

            return broken_segments(), None

    _install_fake_module(monkeypatch, FakeModel)
    provider = FasterWhisperProvider(str(tmp_path))

    with pytest.raises(ProtocolError) as captured:
        provider.transcribe("audio.wav", "zh")

    assert captured.value.error_code == "TRANSCRIPTION_FAILED"


def test_provider_maps_model_load_failure(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    class BrokenModel:
        def __init__(self, _model_path: str, **_kwargs: object) -> None:
            raise RuntimeError("native model load failed")

    _install_fake_module(monkeypatch, BrokenModel)
    provider = FasterWhisperProvider(str(tmp_path))

    with pytest.raises(ProtocolError) as captured:
        provider.transcribe("audio.wav", "zh")

    assert captured.value.error_code == "MODEL_LOAD_FAILED"
