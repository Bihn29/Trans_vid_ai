from __future__ import annotations

import json
from pathlib import Path

import pytest
from jsonschema import Draft202012Validator

from workers.common.protocol import ProtocolError
from workers.separation.contract import EnergyMaskProvider, read_pcm16_mono, write_pcm16_mono
from workers.separation.main import contained_existing


def test_energy_mask_is_deterministic_and_reconstructs_input() -> None:
    source = [100, -100] * 160 + [10_000, -10_000] * 160
    encoded = write_pcm16_mono(source, 16_000)
    first = EnergyMaskProvider().separate(encoded, 0.5)
    second = EnergyMaskProvider().separate(encoded, 0.5)
    assert first == second
    vocals, _ = read_pcm16_mono(first.vocals)
    background, _ = read_pcm16_mono(first.background)
    assert [voice + bed for voice, bed in zip(vocals, background, strict=True)] == source


def test_energy_mask_rejects_invalid_audio_and_threshold() -> None:
    with pytest.raises(ProtocolError):
        EnergyMaskProvider().separate(b"bad", 0.5)
    with pytest.raises(ProtocolError):
        EnergyMaskProvider().separate(write_pcm16_mono([1], 16_000), 0.0)


def test_engine_manifest_is_explicitly_builtin_and_offline() -> None:
    root = Path(__file__).resolve().parents[3]
    manifest = json.loads(
        (root / "resources/manifests/separation-energy-mask.json").read_text(encoding="utf-8")
    )
    assert manifest["approved"] is True
    assert manifest["install"] == {"required": False, "silent_download": False}
    assert manifest["consent"] == {"required": False, "sends_data_off_device": False}
    assert manifest["model"] == {"required": False, "files": []}
    schema = json.loads(
        (root / "schemas/separation-request.schema.json").read_text(encoding="utf-8")
    )
    Draft202012Validator.check_schema(schema)


def test_source_path_traversal_is_rejected(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.chdir(tmp_path)
    with pytest.raises(ProtocolError):
        contained_existing("../outside.wav")
