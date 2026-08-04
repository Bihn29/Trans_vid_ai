from __future__ import annotations

import struct
from typing import Protocol

from workers.common.protocol import ProtocolError


class TtsProvider(Protocol):
    provider_id: str
    sends_data_off_device: bool

    def synthesize(self, text: str, voice_id: str, speed: float) -> bytes: ...


class TemporaryTtsError(Exception):
    pass


def validate_wav(data: bytes) -> tuple[int, int, int, int]:
    if len(data) < 44 or data[:4] != b"RIFF" or data[8:12] != b"WAVE":
        raise ProtocolError("INVALID_TTS_AUDIO", "Âm thanh tổng hợp không phải WAV hợp lệ.")
    offset = 12
    sample_rate = channels = bits = data_size = 0
    while offset + 8 <= len(data):
        kind = data[offset : offset + 4]
        size = struct.unpack_from("<I", data, offset + 4)[0]
        body = offset + 8
        if body + size > len(data):
            raise ProtocolError("INVALID_TTS_AUDIO", "WAV bị cắt ngắn.")
        if kind == b"fmt " and size >= 16:
            audio_format, channels, sample_rate, _, _, bits = struct.unpack_from(
                "<HHIIHH", data, body
            )
            if audio_format != 1:
                raise ProtocolError("INVALID_TTS_AUDIO", "WAV phải là PCM.")
        elif kind == b"data":
            data_size = size
        offset = body + size + (size % 2)
    if not sample_rate or channels not in {1, 2} or bits not in {16, 24, 32} or not data_size:
        raise ProtocolError("INVALID_TTS_AUDIO", "Metadata WAV không hợp lệ.")
    duration_ms = data_size * 8 * 1000 // (sample_rate * channels * bits)
    if duration_ms <= 0:
        raise ProtocolError("INVALID_TTS_AUDIO", "WAV không có thời lượng.")
    return duration_ms, sample_rate, channels, bits


def synthesize_with_retry(
    provider: TtsProvider, text: str, voice: str, speed: float, attempts: int
) -> bytes:
    if not 1 <= attempts <= 3:
        raise ProtocolError("INVALID_RETRY_POLICY", "Chính sách thử lại không hợp lệ.")
    for attempt in range(attempts):
        try:
            audio = provider.synthesize(text, voice, speed)
            validate_wav(audio)
            return audio
        except TemporaryTtsError:
            if attempt + 1 == attempts:
                break
    raise ProtocolError("TTS_PROVIDER_FAILED", "Nhà cung cấp giọng nói tạm thời không khả dụng.")
