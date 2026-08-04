"""Tests for the ASR worker entry point."""

from __future__ import annotations

import hashlib
import json
from io import StringIO
from pathlib import Path
from unittest.mock import patch

import pytest

from workers.asr.contract import AsrSegment
from workers.asr.main import handle
from workers.common.protocol import PROTOCOL_VERSION


class _DeterministicTestProvider:
    def transcribe(self, audio_path: str, language: str) -> list[AsrSegment]:
        assert Path(audio_path).is_file()
        assert language == "zh"
        return [
            AsrSegment(0, 2_000, "你好世界", 0.95),
            AsrSegment(2_000, 4_500, "这是一个测试", 0.88),
        ]


def _make_request(
    action: str = "transcribe",
    model_id: str = "funasr:paraformer-zh",
    audio_path: str = "audio/original/input.wav",
) -> dict[str, object]:
    return {
        "protocol_version": PROTOCOL_VERSION,
        "request_id": "11111111-1111-4111-8111-111111111111",
        "action": action,
        "project_id": "22222222-2222-4222-8222-222222222222",
        "input": {
            "audio_path": audio_path,
            "model_id": model_id,
            "language": "zh",
        },
        "config": {},
        "output_directory": "metadata",
    }


def _prepare_project(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    (tmp_path / "audio" / "original").mkdir(parents=True)
    (tmp_path / "audio" / "original" / "input.wav").write_bytes(b"RIFF-test")
    (tmp_path / "metadata").mkdir()
    monkeypatch.chdir(tmp_path)


def _capture_events(
    request: dict[str, object], *, use_test_provider: bool = True
) -> list[dict[str, object]]:
    events: list[dict[str, object]] = []
    captured = StringIO()

    def mock_emit(payload: dict[str, object], stream: object = None) -> None:
        _ = stream
        serialized = json.dumps(payload, ensure_ascii=False, separators=(",", ":"))
        captured.write(serialized + "\n")
        events.append(dict(payload))

    resolver = (
        patch(
            "workers.asr.main._resolve_provider",
            return_value=_DeterministicTestProvider(),
        )
        if use_test_provider
        else patch("workers.asr.main._resolve_provider", wraps=None)
    )
    with patch("workers.asr.main.emit_event", side_effect=mock_emit):
        if use_test_provider:
            with resolver:
                handle(request)  # type: ignore[arg-type]
        else:
            handle(request)  # type: ignore[arg-type]

    return events


class TestAsrWorker:
    def test_transcription_writes_verified_artifact(
        self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        _prepare_project(tmp_path, monkeypatch)
        events = _capture_events(_make_request())
        completed = next(event for event in events if event["event"] == "completed")
        artifacts = completed["artifacts"]
        assert isinstance(artifacts, list)
        artifact = artifacts[0]
        assert isinstance(artifact, dict)
        output = tmp_path / str(artifact["relative_path"])
        content = output.read_bytes()
        assert artifact["size_bytes"] == len(content)
        assert artifact["sha256"] == hashlib.sha256(content).hexdigest()

    def test_completed_event_has_metrics(
        self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        _prepare_project(tmp_path, monkeypatch)
        events = _capture_events(_make_request())
        completed = next(event for event in events if event["event"] == "completed")
        metrics = completed.get("metrics", {})
        assert isinstance(metrics, dict)
        assert metrics.get("worker") == "asr"
        assert metrics.get("segment_count") == 2

    def test_progress_events_are_emitted(
        self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        _prepare_project(tmp_path, monkeypatch)
        events = _capture_events(_make_request())
        assert any(event.get("event") == "progress" for event in events)

    def test_regional_rerun_limits_output_timestamps(
        self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        _prepare_project(tmp_path, monkeypatch)
        request = _make_request()
        request["config"] = {"region_start_ms": 1_000, "region_end_ms": 3_000}
        events = _capture_events(request)
        completed = next(event for event in events if event["event"] == "completed")
        artifacts = completed["artifacts"]
        assert isinstance(artifacts, list)
        artifact = artifacts[0]
        assert isinstance(artifact, dict)
        payload = json.loads((tmp_path / str(artifact["relative_path"])).read_text("utf-8"))
        assert payload["segments"][0]["start_ms"] == 1_000
        assert payload["segments"][-1]["end_ms"] == 3_000

    def test_unsupported_action_fails(
        self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        _prepare_project(tmp_path, monkeypatch)
        events = _capture_events(_make_request(action="unknown"))
        terminal = [event for event in events if event.get("event") in ("completed", "failed")]
        assert len(terminal) == 1
        assert terminal[0].get("error_code") == "UNSUPPORTED_ACTION"

    def test_unsupported_model_fails(
        self, tmp_path: Path, monkeypatch: pytest.MonkeyPatch
    ) -> None:
        _prepare_project(tmp_path, monkeypatch)
        events = _capture_events(
            _make_request(model_id="nonexistent:model"), use_test_provider=False
        )
        terminal = [event for event in events if event.get("event") in ("completed", "failed")]
        assert len(terminal) == 1
        assert terminal[0].get("error_code") == "UNSUPPORTED_MODEL"

    @pytest.mark.parametrize("audio_path", ["../outside.wav", "C:/outside.wav", "/outside.wav"])
    def test_rejects_audio_path_escape(
        self,
        tmp_path: Path,
        monkeypatch: pytest.MonkeyPatch,
        audio_path: str,
    ) -> None:
        _prepare_project(tmp_path, monkeypatch)
        events = _capture_events(_make_request(audio_path=audio_path))
        failed = next(event for event in events if event["event"] == "failed")
        assert failed["error_code"] == "UNSAFE_PATH"
