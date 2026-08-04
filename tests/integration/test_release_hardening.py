from __future__ import annotations

import json
from pathlib import Path
from typing import Any

from jsonschema import Draft202012Validator, FormatChecker

ROOT = Path(__file__).resolve().parents[2]


def load_json(path: Path) -> dict[str, Any]:
    value = json.loads(path.read_text(encoding="utf-8"))
    assert isinstance(value, dict)
    return value


def test_all_schemas_are_valid_draft_2020_12() -> None:
    schemas = sorted((ROOT / "schemas").glob("*.schema.json"))
    assert len(schemas) == 17
    for path in schemas:
        Draft202012Validator.check_schema(load_json(path))


def test_approved_model_catalog_is_schema_valid_and_never_bundles_weights() -> None:
    schema = load_json(ROOT / "schemas/approved-model-manifest.schema.json")
    manifests = sorted((ROOT / "resources/manifests/models").glob("*.json"))
    assert manifests
    for path in manifests:
        Draft202012Validator(
            schema, format_checker=FormatChecker()
        ).validate(load_json(path))
    unexpected = [
        path
        for path in (ROOT / "resources/manifests/models").rglob("*")
        if path.is_file() and path.suffix != ".json"
    ]
    assert unexpected == []


def test_installer_is_explicit_current_user_nsis_without_updater() -> None:
    config = load_json(ROOT / "apps/desktop/src-tauri/tauri.conf.json")
    bundle = config["bundle"]
    assert bundle["targets"] == ["nsis"]
    assert bundle["windows"]["nsis"]["installMode"] == "currentUser"
    assert bundle["windows"]["nsis"]["compression"] == "zlib"
    assert bundle["windows"]["webviewInstallMode"] == {"type": "skip"}
    assert "updater" not in json.dumps(config).lower()
    workflow = (ROOT / ".github/workflows/release.yml").read_text(encoding="utf-8")
    assert "clean-machine-acceptance:" in workflow
    assert "VietDub Studio/vietdub-studio.exe" in workflow
    assert workflow.count("signtool verify /pa /v") >= 4


def test_release_manifest_schema_requires_signature_and_disables_updates() -> None:
    schema = load_json(ROOT / "schemas/release-manifest.schema.json")
    assert schema["properties"]["authenticodeRequired"] == {"const": True}
    assert schema["properties"]["automaticUpdates"] == {"const": False}
