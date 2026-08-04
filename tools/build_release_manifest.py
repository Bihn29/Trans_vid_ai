from __future__ import annotations

import argparse
import hashlib
import json
import re
from pathlib import Path


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        while chunk := stream.read(64 * 1024):
            digest.update(chunk)
    return digest.hexdigest()


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--artifact", type=Path, required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--channel", choices=("stable", "beta"), default="stable")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    artifact = args.artifact.resolve(strict=True)
    if artifact.suffix.lower() not in {".exe", ".msi"} or not re.fullmatch(
        r"[0-9A-Za-z][0-9A-Za-z.+-]{0,63}", args.version
    ):
        raise ValueError("invalid release artifact or version")
    manifest = {
        "schemaVersion": 1,
        "product": "VietDub Studio",
        "version": args.version,
        "channel": args.channel,
        "artifactFilename": artifact.name,
        "sha256": sha256_file(artifact),
        "sizeBytes": artifact.stat().st_size,
        "authenticodeRequired": True,
        "automaticUpdates": False,
    }
    args.output.write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
        newline="\n",
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
