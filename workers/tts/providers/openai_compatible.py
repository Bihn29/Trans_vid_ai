from __future__ import annotations

import ipaddress
import json
import urllib.error
import urllib.parse
import urllib.request
from typing import Protocol, cast

from workers.common.protocol import JsonObject, ProtocolError
from workers.tts.contract import TemporaryTtsError

APPROVED_VOICES = frozenset({"alloy", "nova"})


class AudioTransport(Protocol):
    def post(self, url: str, headers: dict[str, str], payload: JsonObject) -> bytes: ...


class _NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, *args: object, **kwargs: object) -> None:
        return None


class StdlibAudioTransport:
    def post(self, url: str, headers: dict[str, str], payload: JsonObject) -> bytes:
        request = urllib.request.Request(  # noqa: S310 -- constructor receives validated HTTPS
            url, data=json.dumps(payload).encode(), headers=headers, method="POST"
        )
        try:
            with urllib.request.build_opener(_NoRedirect()).open(request, timeout=90) as response:
                data = cast(bytes, response.read(32 * 1024 * 1024 + 1))
        except (urllib.error.URLError, TimeoutError) as error:
            raise TemporaryTtsError from error
        if len(data) > 32 * 1024 * 1024:
            raise ProtocolError("TTS_AUDIO_TOO_LARGE", "Âm thanh tổng hợp vượt giới hạn.")
        return data


class OpenAiCompatibleTtsAdapter:
    provider_id = "openai-compatible"
    sends_data_off_device = True

    def __init__(
        self, endpoint: str, model: str, api_key: str, transport: AudioTransport | None = None
    ) -> None:
        parsed = urllib.parse.urlsplit(endpoint)
        if (
            parsed.scheme != "https"
            or not parsed.hostname
            or parsed.username
            or parsed.password
            or parsed.fragment
            or parsed.port not in {None, 443}
        ):
            raise ProtocolError("INVALID_PROVIDER_ENDPOINT", "Endpoint TTS không an toàn.")
        try:
            address = ipaddress.ip_address(parsed.hostname)
        except ValueError:
            address = None
        if address is not None and not address.is_global:
            raise ProtocolError("INVALID_PROVIDER_ENDPOINT", "Endpoint TTS không an toàn.")
        if not model or not api_key:
            raise ProtocolError("INVALID_PROVIDER_CONFIG", "Cấu hình TTS không hợp lệ.")
        self.endpoint = endpoint
        self.model = model
        self._key = api_key
        self._transport = transport or StdlibAudioTransport()

    def synthesize(self, text: str, voice_id: str, speed: float) -> bytes:
        if (
            voice_id not in APPROVED_VOICES
            or not text.strip()
            or len(text) > 4096
            or not 0.25 <= speed <= 4.0
        ):
            raise ProtocolError("INVALID_TTS_REQUEST", "Yêu cầu TTS không hợp lệ.")
        return self._transport.post(
            self.endpoint,
            {"Authorization": f"Bearer {self._key}", "Content-Type": "application/json"},
            {
                "model": self.model,
                "input": text,
                "voice": voice_id,
                "response_format": "wav",
                "speed": speed,
            },
        )
