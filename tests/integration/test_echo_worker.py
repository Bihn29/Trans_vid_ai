from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path
from typing import cast
from uuid import uuid4

import pytest

from workers.common.protocol import JsonObject, validate_event

_REPOSITORY_ROOT = Path(__file__).resolve().parents[2]
_WORKER = _REPOSITORY_ROOT / "workers" / "echo" / "main.py"


@pytest.mark.integration
def test_real_echo_worker_json_lines_exchange() -> None:
    request_id = str(uuid4())
    request: JsonObject = {
        "protocol_version": 1,
        "request_id": request_id,
        "action": "echo",
        "project_id": str(uuid4()),
        "input": {"value": "deterministic"},
        "config": {},
        "output_directory": "metadata/echo",
    }

    completed = subprocess.run(
        [sys.executable, "-u", str(_WORKER)],
        input=json.dumps(request) + "\n",
        text=True,
        capture_output=True,
        timeout=5,
        check=False,
        cwd=_REPOSITORY_ROOT,
    )

    assert completed.returncode == 0
    assert completed.stderr == ""
    events = [cast(JsonObject, json.loads(line)) for line in completed.stdout.splitlines()]
    for event in events:
        validate_event(event)
        assert event["request_id"] == request_id
    assert [event["event"] for event in events] == ["progress", "progress", "completed"]
    assert cast(dict[str, object], events[-1]["metrics"])["echo"] == {"value": "deterministic"}

