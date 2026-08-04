"""Deterministic ASR worker fixture; never packaged with production workers."""

from __future__ import annotations

import hashlib
import json
import os
import sys
from pathlib import Path


def emit(payload: dict[str, object]) -> None:
    sys.stdout.write(json.dumps(payload, ensure_ascii=False, separators=(",", ":")) + "\n")
    sys.stdout.flush()


request = json.loads(sys.stdin.readline())
request_id = request["request_id"]
output_directory = Path(request["output_directory"])
relative_path = output_directory / f"transcript-{request_id}.json"
payload = {
    "schema_version": 1,
    "segments": [
        {
            "start_ms": 0,
            "end_ms": 2000,
            "text": "你好世界",
            "confidence": 0.95,
            "speaker_label": None,
        },
        {
            "start_ms": 2000,
            "end_ms": 4500,
            "text": "这是一个测试",
            "confidence": 0.88,
            "speaker_label": None,
        },
    ],
}
region_start = request["config"].get("region_start_ms")
region_end = request["config"].get("region_end_ms")
if isinstance(region_start, int) and isinstance(region_end, int):
    payload["segments"] = [
        {
            **segment,
            "start_ms": max(segment["start_ms"], region_start),
            "end_ms": min(segment["end_ms"], region_end),
        }
        for segment in payload["segments"]
        if segment["start_ms"] < region_end and segment["end_ms"] > region_start
    ]
content = json.dumps(payload, ensure_ascii=False, indent=2).encode("utf-8")
with relative_path.open("xb") as output_file:
    output_file.write(content)
    output_file.flush()
    os.fsync(output_file.fileno())

emit(
    {
        "protocol_version": 1,
        "request_id": request_id,
        "event": "progress",
        "progress": 100,
        "message": "Deterministic ASR fixture completed",
    }
)
emit(
    {
        "protocol_version": 1,
        "request_id": request_id,
        "event": "completed",
        "artifacts": [
            {
                "type": "transcript",
                "relative_path": relative_path.as_posix(),
                "sha256": hashlib.sha256(content).hexdigest(),
                "size_bytes": len(content),
                "metadata": {"segment_count": 2},
            }
        ],
        "metrics": {"worker": "deterministic-test-fixture", "segment_count": 2},
        "warnings": [],
    }
)
