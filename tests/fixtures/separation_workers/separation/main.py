from __future__ import annotations

import json
import sys

request = json.loads(sys.stdin.readline())
print(
    json.dumps(
        {
            "protocol_version": 1,
            "request_id": request["request_id"],
            "event": "failed",
            "error_code": "TEST_SEPARATION_FAILURE",
            "safe_message": "test separation failure",
        },
        separators=(",", ":"),
    ),
    flush=True,
)
