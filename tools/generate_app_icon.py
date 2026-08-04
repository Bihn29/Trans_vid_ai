"""Generate the deterministic, clean-room VietDub Studio Windows icon."""

from __future__ import annotations

import math
import struct
import zlib
from pathlib import Path

_SIZES = (32, 48, 64, 128, 256)
_OUTPUT = (
    Path(__file__).resolve().parents[1]
    / "apps"
    / "desktop"
    / "src-tauri"
    / "icons"
    / "icon.ico"
)
_PNG_OUTPUT = _OUTPUT.with_suffix(".png")


def _inside_rounded_square(x: float, y: float, size: int) -> bool:
    margin = size * 0.07
    radius = size * 0.22
    nearest_x = min(max(x, margin + radius), size - margin - radius)
    nearest_y = min(max(y, margin + radius), size - margin - radius)
    return math.hypot(x - nearest_x, y - nearest_y) <= radius


def _distance_to_segment(
    x: float,
    y: float,
    start_x: float,
    start_y: float,
    end_x: float,
    end_y: float,
) -> float:
    delta_x = end_x - start_x
    delta_y = end_y - start_y
    length_squared = delta_x * delta_x + delta_y * delta_y
    projection = ((x - start_x) * delta_x + (y - start_y) * delta_y) / length_squared
    projection = min(1.0, max(0.0, projection))
    closest_x = start_x + projection * delta_x
    closest_y = start_y + projection * delta_y
    return math.hypot(x - closest_x, y - closest_y)


def _pixel(x: int, y: int, size: int) -> tuple[int, int, int, int]:
    center_x = x + 0.5
    center_y = y + 0.5
    if not _inside_rounded_square(center_x, center_y, size):
        return (0, 0, 0, 0)

    blend = (center_x + center_y) / (size * 2)
    red = round(119 * (1 - blend) + 50 * blend)
    green = round(91 * (1 - blend) + 202 * blend)
    blue = round(224 * (1 - blend) + 180 * blend)

    stroke = size * 0.075
    left = _distance_to_segment(
        center_x,
        center_y,
        size * 0.27,
        size * 0.29,
        size * 0.48,
        size * 0.71,
    )
    right = _distance_to_segment(
        center_x,
        center_y,
        size * 0.48,
        size * 0.71,
        size * 0.74,
        size * 0.25,
    )
    if min(left, right) <= stroke:
        return (250, 252, 255, 255)
    return (red, green, blue, 255)


def _png(size: int) -> bytes:
    scanlines = bytearray()
    for y in range(size):
        scanlines.append(0)
        for x in range(size):
            scanlines.extend(_pixel(x, y, size))

    def chunk(kind: bytes, data: bytes) -> bytes:
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))

    signature = b"\x89PNG\r\n\x1a\n"
    header = struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0)
    return (
        signature
        + chunk(b"IHDR", header)
        + chunk(b"IDAT", zlib.compress(scanlines, 9))
        + chunk(b"IEND", b"")
    )


def generate_icon(output: Path = _OUTPUT, png_output: Path = _PNG_OUTPUT) -> None:
    images = [_png(size) for size in _SIZES]
    directory_size = 6 + 16 * len(images)
    offset = directory_size
    entries = bytearray()
    for size, image in zip(_SIZES, images, strict=True):
        dimension = 0 if size == 256 else size
        entries.extend(
            struct.pack(
                "<BBBBHHII",
                dimension,
                dimension,
                0,
                0,
                1,
                32,
                len(image),
                offset,
            )
        )
        offset += len(image)

    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_bytes(struct.pack("<HHH", 0, 1, len(images)) + entries + b"".join(images))
    png_output.write_bytes(images[-1])


if __name__ == "__main__":
    generate_icon()
