from __future__ import annotations

import hashlib
import json
import struct
import sys
import time
from pathlib import Path


def emit(value: dict[str, object]) -> None:
    print(json.dumps(value, separators=(",", ":")), flush=True)


def pcm_wav(duration_ms: int) -> bytes:
    sample_rate = 16_000
    sample_count = sample_rate * duration_ms // 1_000
    audio = b"\0\0" * sample_count
    byte_rate = sample_rate * 2
    return b"".join(
        [
            b"RIFF",
            struct.pack("<I", 36 + len(audio)),
            b"WAVEfmt ",
            struct.pack("<IHHIIHH", 16, 1, 1, sample_rate, byte_rate, 2, 16),
            b"data",
            struct.pack("<I", len(audio)),
            audio,
        ]
    )


request = json.loads(sys.stdin.readline())
request_id = request["request_id"]
data = request["input"]["tts"]
text = data["text"]

if "FAIL_ONCE" in text:
    marker = Path("metadata") / f"tts-fail-{data['segment_id']}"
    if not marker.exists():
        marker.write_text("1", encoding="utf-8")
        emit(
            {
                "protocol_version": 1,
                "request_id": request_id,
                "event": "failed",
                "error_code": "TEST_FAILURE",
                "safe_message": "test failure",
            }
        )
        raise SystemExit(0)

if "SLEEP" in text:
    time.sleep(10)

duration_ms = 2_500 if "LONG" in text else 1_000
audio = pcm_wav(duration_ms)
directory = "previews" if request["action"] == "synthesize_preview" else "audio/tts"
relative_path = f"{directory}/tts-{request_id}.wav"
Path(relative_path).write_bytes(audio)

emit(
    {
        "protocol_version": 1,
        "request_id": request_id,
        "event": "completed",
        "artifacts": [
            {
                "type": ("preview" if request["action"] == "synthesize_preview" else "tts"),
                "relative_path": relative_path,
                "sha256": hashlib.sha256(audio).hexdigest(),
                "size_bytes": len(audio),
                "metadata": {
                    "duration_ms": duration_ms,
                    "sample_rate": 16_000,
                    "channels": 1,
                    "bits_per_sample": 16,
                    "voice_id": data["voice_id"],
                },
            }
        ],
        "metrics": {"test_only": True},
        "warnings": [],
    }
)
