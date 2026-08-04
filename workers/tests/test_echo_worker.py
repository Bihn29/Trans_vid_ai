from __future__ import annotations

from uuid import uuid4

import pytest

from workers.common.protocol import JsonObject
from workers.echo.main import handle


def test_echo_handle_emits_progress_and_completion(monkeypatch: pytest.MonkeyPatch) -> None:
    events: list[JsonObject] = []
    monkeypatch.setattr("workers.echo.main.emit_event", events.append)
    request_id = str(uuid4())

    handle(
        {
            "protocol_version": 1,
            "request_id": request_id,
            "action": "echo",
            "project_id": str(uuid4()),
            "input": {"value": 7},
            "config": {},
            "output_directory": "metadata/echo",
        }
    )

    assert [event["event"] for event in events] == ["progress", "progress", "completed"]
    assert events[-1]["request_id"] == request_id
