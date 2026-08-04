from __future__ import annotations

import argparse
import importlib.metadata
import json
import os
import re
import shutil
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
BANNED_LICENSES = ("AGPL", "SSPL", "BUSL", "BUSINESS SOURCE", "ELASTIC LICENSE")
PYTHON_LICENSE_OVERRIDES = {
    # The Windows wheel omits Core Metadata license fields. The exact version and
    # upstream Apache-2.0 source review are recorded in the dependency inventory.
    ("sentencepiece", "0.2.1"): "Apache-2.0",
}


@dataclass(frozen=True, order=True)
class Component:
    ecosystem: str
    name: str
    version: str
    license: str

    @property
    def purl(self) -> str:
        kind = {"cargo": "cargo", "npm": "npm", "python": "pypi"}[self.ecosystem]
        return f"pkg:{kind}/{self.name}@{self.version}"


def normalized_license(value: str | None) -> str:
    license_value = (value or "").strip().replace("\n", " ")
    if not license_value or license_value.upper() in {"UNKNOWN", "NONE", "NOASSERTION"}:
        raise RuntimeError("dependency has no reviewable license metadata")
    upper = license_value.upper()
    if any(token in upper for token in BANNED_LICENSES):
        raise RuntimeError(f"blocked dependency license: {license_value}")
    if "GPL" in upper and "LGPL" not in upper:
        raise RuntimeError(f"GPL dependency requires written approval: {license_value}")
    return license_value[:512]


def cargo_components() -> list[Component]:
    cargo = os.environ.get("CARGO") or shutil.which("cargo")
    if cargo is None and sys.platform == "win32":
        default_cargo = Path.home() / ".cargo" / "bin" / "cargo.exe"
        if default_cargo.is_file():
            cargo = str(default_cargo)
    if cargo is None:
        raise RuntimeError("cargo must be on PATH for release audit")
    result = subprocess.run(  # noqa: S603 - resolved absolute executable, fixed arguments
        [
            cargo,
            "metadata",
            "--locked",
            "--filter-platform",
            "x86_64-pc-windows-msvc",
            "--format-version",
            "1",
        ],
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
        encoding="utf-8",
    )
    metadata: dict[str, Any] = json.loads(result.stdout)
    components = []
    for package in metadata["packages"]:
        if package.get("source") is None:
            continue
        components.append(
            Component(
                "cargo",
                str(package["name"]),
                str(package["version"]),
                normalized_license(package.get("license")),
            )
        )
    return components


def pnpm_locked_packages() -> set[tuple[str, str]]:
    source = (ROOT / "pnpm-lock.yaml").read_text(encoding="utf-8")
    packages = source.split("\npackages:\n", 1)[1].split("\nsnapshots:\n", 1)[0]
    locked: set[tuple[str, str]] = set()
    for line in packages.splitlines():
        match = re.fullmatch(r"  '?(.+@[^:']+)'?:", line)
        if match:
            name, version = match.group(1).rsplit("@", 1)
            locked.add((name, version))
    if not locked:
        raise RuntimeError("pnpm lockfile has no packages")
    return locked


def npm_components() -> list[Component]:
    store = ROOT / "node_modules" / ".pnpm"
    if not store.is_dir():
        raise RuntimeError("pnpm install must run before release audit")
    components: dict[tuple[str, str], Component] = {}
    locked = pnpm_locked_packages()
    for manifest_path in store.glob("*/node_modules/*/package.json"):
        data = json.loads(manifest_path.read_text(encoding="utf-8"))
        name = data.get("name")
        version = data.get("version")
        if not isinstance(name, str) or not isinstance(version, str):
            continue
        if (name, version) not in locked:
            continue
        license_field = data.get("license")
        if isinstance(license_field, dict):
            license_field = license_field.get("type")
        license_value = normalized_license(
            license_field if isinstance(license_field, str) else None
        )
        components[(name, version)] = Component("npm", name, version, license_value)
    for manifest_path in store.glob("*/node_modules/@*/*/package.json"):
        data = json.loads(manifest_path.read_text(encoding="utf-8"))
        name = data.get("name")
        version = data.get("version")
        if not isinstance(name, str) or not isinstance(version, str):
            continue
        if (name, version) not in locked:
            continue
        license_field = data.get("license")
        if isinstance(license_field, dict):
            license_field = license_field.get("type")
        license_value = normalized_license(
            license_field if isinstance(license_field, str) else None
        )
        components[(name, version)] = Component("npm", name, version, license_value)
    direct = json.loads((ROOT / "apps" / "desktop" / "package.json").read_text(encoding="utf-8"))
    for section in ("dependencies", "devDependencies"):
        for name, version in direct[section].items():
            if (name, version) not in components:
                raise RuntimeError(f"pnpm component is not reconciled: {name} {version}")
    return list(components.values())


def python_components() -> list[Component]:
    required = {}
    for line in (ROOT / "requirements-dev.txt").read_text(encoding="utf-8").splitlines():
        match = re.fullmatch(r"([A-Za-z0-9_.-]+)==([^\s]+)", line.strip())
        if match:
            required[match.group(1).lower().replace("_", "-")] = match.group(2)
    components: list[Component] = []
    for name, version in sorted(required.items()):
        distribution = importlib.metadata.distribution(name)
        if distribution.version != version:
            raise RuntimeError(f"Python package version mismatch: {name}")
        metadata = distribution.metadata.json
        license_value_raw = metadata.get("license_expression")
        if isinstance(license_value_raw, list):
            license_value: str | None = " OR ".join(license_value_raw)
        elif isinstance(license_value_raw, str):
            license_value = license_value_raw
        else:
            license_value = None
        if not license_value:
            legacy_license = metadata.get("license")
            if isinstance(legacy_license, list):
                license_value = " OR ".join(legacy_license)
            elif isinstance(legacy_license, str):
                license_value = legacy_license
        if not license_value:
            classifiers = distribution.metadata.get_all("Classifier") or []
            license_value = next(
                (
                    value.rsplit("::", 1)[-1].strip()
                    for value in classifiers
                    if "License ::" in value
                ),
                None,
            )
        if not license_value:
            license_value = PYTHON_LICENSE_OVERRIDES.get((name, version))
        components.append(
            Component("python", name, version, normalized_license(license_value))
        )
    return components


def build_outputs() -> tuple[str, str]:
    components = sorted(cargo_components() + npm_components() + python_components())
    unique = {(item.ecosystem, item.name, item.version): item for item in components}
    if len(unique) != len(components):
        raise RuntimeError("duplicate SBOM component")
    sbom: dict[str, Any] = {
        "bomFormat": "CycloneDX",
        "specVersion": "1.6",
        "version": 1,
        "metadata": {
            "component": {
                "type": "application",
                "name": "VietDub Studio",
                "version": "0.1.0",
            }
        },
        "components": [
            {
                "type": "library",
                "group": item.ecosystem,
                "name": item.name,
                "version": item.version,
                "purl": item.purl,
                "licenses": [{"expression": item.license}],
            }
            for item in components
        ],
    }
    sbom_text = json.dumps(sbom, ensure_ascii=False, indent=2, sort_keys=True) + "\n"
    rows = [
        "# Third-party notices",
        "",
        "Generated from committed manifests/lockfiles and installed package metadata.",
        "VietDub Studio does not bundle FFmpeg, yt-dlp, cloud SDKs, or model weights.",
        "",
        "| Ecosystem | Component | Version | License |",
        "| --- | --- | --- | --- |",
    ]
    rows.extend(
        f"| {item.ecosystem} | {item.name} | {item.version} | {item.license.replace('|', '/')} |"
        for item in components
    )
    return sbom_text, "\n".join(rows) + "\n"


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    sbom, notices = build_outputs()
    outputs = {
        ROOT / "docs" / "release" / "sbom.cdx.json": sbom,
        ROOT / "docs" / "release" / "THIRD_PARTY_NOTICES.md": notices,
    }
    if args.check:
        stale = [
            path
            for path, value in outputs.items()
            if not path.is_file() or path.read_text(encoding="utf-8") != value
        ]
        if stale:
            paths = ", ".join(str(path.relative_to(ROOT)) for path in stale)
            print("release metadata is stale: " + paths, file=sys.stderr)
            return 1
    else:
        for path, value in outputs.items():
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(value, encoding="utf-8", newline="\n")
    print(f"release audit passed: {len(json.loads(sbom)['components'])} components")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
