"""Deterministic translation worker used only by Rust integration tests."""

from __future__ import annotations

import hashlib
import json
import sys
import time
from pathlib import Path


def emit(payload: dict[str, object]) -> None:
    print(json.dumps(payload, separators=(",", ":")), flush=True)


request = json.loads(sys.stdin.readline())
request_id = request["request_id"]
block = request["input"]["block"]
segments = block["segments"]
source = " ".join(segment["source_text"] for segment in segments)

if "SLEEP" in source:
    time.sleep(10)

if "FAIL_ONCE" in source:
    marker = Path("metadata") / f"fail-once-{segments[0]['id']}"
    if not marker.exists():
        marker.write_text("failed", encoding="utf-8")
        emit(
            {
                "protocol_version": 1,
                "request_id": request_id,
                "event": "failed",
                "error_code": "TEST_PROVIDER_FAILURE",
                "safe_message": "Deterministic test failure.",
            }
        )
        raise SystemExit(0)

translations = [
    {"id": segment["id"], "text": f"vi:{segment['source_text']}"} for segment in segments
]
result = {"schema_version": 1, "translations": translations}
content = json.dumps(result, ensure_ascii=False, separators=(",", ":")).encode("utf-8")
relative = f"metadata/translation-{request_id}.json"
Path(relative).write_bytes(content)
emit(
    {
        "protocol_version": 1,
        "request_id": request_id,
        "event": "completed",
        "artifacts": [
            {
                "type": "translation_block",
                "relative_path": relative,
                "sha256": hashlib.sha256(content).hexdigest(),
                "size_bytes": len(content),
                "metadata": {"test_only": True},
            }
        ],
        "metrics": {"provider": "test-only"},
        "warnings": [],
    }
)
