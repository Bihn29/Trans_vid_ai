from __future__ import annotations

import json
import os
from pathlib import Path
from unittest.mock import patch

from workers.common.protocol import JsonObject
from workers.translation import main as worker

ID = "11111111-1111-4111-8111-111111111111"


class _TestOnlyProvider:
    provider_id = "test-only"
    sends_data_off_device = False

    def translate(self, _system: str, _user: str) -> str:
        return json.dumps({"schema_version": 1, "translations": [{"id": ID, "text": "Xin chào"}]})


def request() -> JsonObject:
    return {
        "protocol_version": 1,
        "request_id": "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
        "action": "translate_block",
        "project_id": "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
        "input": {
            "block": {
                "schema_version": 1,
                "source_language": "zh",
                "target_language": "vi",
                "segments": [{"id": ID, "source_text": "你好"}],
                "context_before": [],
                "context_after": [],
                "glossary": [],
                "locked_names": [],
            }
        },
        "config": {"provider_id": "test-only", "max_attempts": 2},
        "output_directory": "metadata",
    }


def test_worker_writes_strict_result_artifact(tmp_path: Path) -> None:
    (tmp_path / "metadata").mkdir()
    previous = Path.cwd()
    os.chdir(tmp_path)
    try:
        events: list[JsonObject] = []
        with (
            patch("workers.translation.main._resolve_provider", return_value=_TestOnlyProvider()),
            patch("workers.translation.main.emit_event", side_effect=events.append),
        ):
            worker.handle(request())
    finally:
        os.chdir(previous)
    completed = events[-1]
    artifacts = completed["artifacts"]
    assert isinstance(artifacts, list)
    artifact = artifacts[0]
    assert isinstance(artifact, dict)
    relative = artifact["relative_path"]
    assert isinstance(relative, str)
    result = json.loads((tmp_path / Path(*relative.split("/"))).read_text(encoding="utf-8"))
    assert result["translations"][0]["id"] == ID
    assert result["translations"][0]["text"] == "Xin chào"
