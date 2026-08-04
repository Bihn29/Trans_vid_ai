from __future__ import annotations

import io
import math
import struct
import wave
from dataclasses import dataclass
from typing import Protocol

from workers.common.protocol import ProtocolError


@dataclass(frozen=True)
class SeparatedAudio:
    vocals: bytes
    background: bytes
    duration_ms: int
    sample_rate: int


class SeparationProvider(Protocol):
    engine_id: str
    sends_data_off_device: bool

    def separate(self, audio: bytes, energy_threshold: float) -> SeparatedAudio: ...


def read_pcm16_mono(data: bytes) -> tuple[list[int], int]:
    try:
        with wave.open(io.BytesIO(data), "rb") as source:
            if (
                source.getnchannels() != 1
                or source.getsampwidth() != 2
                or source.getcomptype() != "NONE"
                or not 8_000 <= source.getframerate() <= 48_000
                or source.getnframes() == 0
                or source.getnframes() > source.getframerate() * 60 * 60 * 6
            ):
                raise ProtocolError("INVALID_AUDIO", "Âm thanh nguồn không hợp lệ.")
            frames = source.readframes(source.getnframes())
            rate = source.getframerate()
    except (EOFError, wave.Error) as error:
        raise ProtocolError("INVALID_AUDIO", "Âm thanh nguồn không hợp lệ.") from error
    if len(frames) % 2:
        raise ProtocolError("INVALID_AUDIO", "Âm thanh nguồn không hợp lệ.")
    samples = list(struct.unpack(f"<{len(frames) // 2}h", frames))
    return samples, rate


def write_pcm16_mono(samples: list[int], sample_rate: int) -> bytes:
    output = io.BytesIO()
    with wave.open(output, "wb") as destination:
        destination.setnchannels(1)
        destination.setsampwidth(2)
        destination.setframerate(sample_rate)
        destination.writeframes(struct.pack(f"<{len(samples)}h", *samples))
    return output.getvalue()


class EnergyMaskProvider:
    engine_id = "energy-mask-v1"
    sends_data_off_device = False

    def separate(self, audio: bytes, energy_threshold: float) -> SeparatedAudio:
        if not math.isfinite(energy_threshold) or not 0.01 <= energy_threshold <= 0.95:
            raise ProtocolError("INVALID_SEPARATION_CONFIG", "Cấu hình tách âm không hợp lệ.")
        samples, sample_rate = read_pcm16_mono(audio)
        frame_size = max(1, sample_rate // 50)
        energies = [
            math.sqrt(
                sum(sample * sample for sample in samples[i : i + frame_size])
                / len(samples[i : i + frame_size])
            )
            for i in range(0, len(samples), frame_size)
        ]
        cutoff = max(energies) * energy_threshold
        masks = [energy >= cutoff and energy > 0 for energy in energies]
        vocals: list[int] = []
        background: list[int] = []
        for index, sample in enumerate(samples):
            if masks[min(index // frame_size, len(masks) - 1)]:
                vocals.append(sample)
                background.append(0)
            else:
                vocals.append(0)
                background.append(sample)
        return SeparatedAudio(
            vocals=write_pcm16_mono(vocals, sample_rate),
            background=write_pcm16_mono(background, sample_rate),
            duration_ms=len(samples) * 1_000 // sample_rate,
            sample_rate=sample_rate,
        )
