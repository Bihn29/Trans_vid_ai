"""Validate every committed JSON Schema using Draft 2020-12."""

from __future__ import annotations

import json
from pathlib import Path
from typing import Any

from jsonschema import Draft202012Validator

ROOT = Path(__file__).resolve().parents[1]


def main() -> int:
    paths = sorted((ROOT / "schemas").glob("*.schema.json"))
    if not paths:
        raise RuntimeError("no schemas found")
    for path in paths:
        schema: dict[str, Any] = json.loads(path.read_text(encoding="utf-8"))
        Draft202012Validator.check_schema(schema)
    print(f"validated {len(paths)} Draft 2020-12 schemas")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
