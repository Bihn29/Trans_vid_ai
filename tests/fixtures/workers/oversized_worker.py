"""Test-only worker that deliberately violates the maximum JSONL message size."""

from __future__ import annotations

import json
import sys


def main() -> int:
    request = json.loads(sys.stdin.readline())
    event = {
        "protocol_version": 1,
        "request_id": request["request_id"],
        "event": "progress",
        "progress": 1,
        "message": "x" * 4096,
    }
    sys.stdout.write(json.dumps(event) + "\n")
    sys.stdout.flush()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

