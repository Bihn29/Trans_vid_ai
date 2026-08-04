from __future__ import annotations

import io
import json
import wave
from pathlib import Path

import pytest
from jsonschema import Draft202012Validator, FormatChecker

from workers.common.protocol import JsonObject, ProtocolError
from workers.tts.contract import TemporaryTtsError, synthesize_with_retry, validate_wav
from workers.tts.providers.openai_compatible import OpenAiCompatibleTtsAdapter


def wav(duration_ms: int = 1000, rate: int = 16000) -> bytes:
    output = io.BytesIO()
    with wave.open(output, "wb") as stream:
        stream.setnchannels(1)
        stream.setsampwidth(2)
        stream.setframerate(rate)
        stream.writeframes(b"\0\0" * (rate * duration_ms // 1000))
    return output.getvalue()


def test_wav_metadata_and_corruption() -> None:
    assert validate_wav(wav()) == (1000, 16000, 1, 16)
    with pytest.raises(ProtocolError):
        validate_wav(b"not wav")


class RetryProvider:
    provider_id = "test-only"
    sends_data_off_device = False

    def __init__(self, failures: int) -> None:
        self.failures = failures
        self.calls = 0

    def synthesize(self, _text: str, _voice: str, _speed: float) -> bytes:
        self.calls += 1
        if self.calls <= self.failures:
            raise TemporaryTtsError
        return wav()


def test_retry_is_bounded() -> None:
    provider = RetryProvider(1)
    assert synthesize_with_retry(provider, "xin chào", "alloy", 1.0, 2)
    assert provider.calls == 2
    failed = RetryProvider(3)
    with pytest.raises(ProtocolError):
        synthesize_with_retry(failed, "xin chào", "alloy", 1.0, 2)
    assert failed.calls == 2


class Transport:
    def __init__(self) -> None:
        self.payload: JsonObject = {}
        self.headers: dict[str, str] = {}

    def post(self, _url: str, headers: dict[str, str], payload: JsonObject) -> bytes:
        self.payload = payload
        self.headers = headers
        return wav()


def test_openai_adapter_uses_wav_and_approved_voices() -> None:
    transport = Transport()
    adapter = OpenAiCompatibleTtsAdapter(
        "https://api.example.com/v1/audio/speech", "tts-1", "secret", transport
    )
    assert adapter.synthesize("xin chào", "alloy", 1.0)
    assert transport.payload["response_format"] == "wav"
    assert transport.headers["Authorization"] == "Bearer secret"
    with pytest.raises(ProtocolError):
        adapter.synthesize("text", "custom-cloned-voice", 1.0)


@pytest.mark.parametrize(
    "endpoint",
    [
        "http://api.example.com/v1/audio/speech",
        "https://127.0.0.1/v1/audio/speech",
        "https://user:pass@api.example.com/v1/audio/speech",
    ],
)
def test_unsafe_endpoints_are_rejected(endpoint: str) -> None:
    with pytest.raises(ProtocolError):
        OpenAiCompatibleTtsAdapter(endpoint, "tts-1", "secret", Transport())


def test_approved_voice_manifest_matches_catalog_schema() -> None:
    root = Path(__file__).resolve().parents[3]
    schema = json.loads(
        (root / "schemas" / "voice-catalog.schema.json").read_text(encoding="utf-8")
    )
    manifest = json.loads(
        (root / "resources" / "manifests" / "tts-openai-compatible.json").read_text(
            encoding="utf-8"
        )
    )
    Draft202012Validator(schema, format_checker=FormatChecker()).validate(manifest)
    assert {voice["voice_id"] for voice in manifest["voices"]} == {"alloy", "nova"}
