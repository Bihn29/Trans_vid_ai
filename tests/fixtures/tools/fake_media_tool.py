from __future__ import annotations

import json
import subprocess
import sys
import time
from pathlib import Path


def main() -> int:
    mode = sys.argv[1]
    if mode == "stdout":
        print("tool-ok")
        return 0
    if mode == "stderr":
        print("Authorization: Bearer fixture-secret", file=sys.stderr)
        return 0
    if mode == "oversize":
        sys.stdout.write("x" * 20_000)
        return 0
    if mode == "sleep":
        time.sleep(5)
        return 0
    if mode == "spawn-child":
        subprocess.Popen(
            [sys.executable, __file__, "delayed-write", sys.argv[2]],
            close_fds=True,
        )
        time.sleep(5)
        return 0
    if mode == "delayed-write":
        time.sleep(1)
        Path(sys.argv[2]).write_text("descendant-survived", encoding="utf-8")
        return 0
    if mode == "fail":
        print("controlled failure", file=sys.stderr)
        return 7
    if mode == "probe":
        print(
            json.dumps(
                {
                    "streams": [
                        {
                            "codec_type": "video",
                            "codec_name": "h264",
                            "width": 640,
                            "height": 360,
                            "avg_frame_rate": "25/1",
                        },
                        {"codec_type": "audio", "codec_name": "aac"},
                    ],
                    "format": {"duration": "2.500", "format_name": "mov,mp4"},
                },
                separators=(",", ":"),
            )
        )
        return 0
    if mode == "probe-invalid":
        print("{}")
        return 0
    if mode == "ffmpeg":
        Path(sys.argv[-1]).write_bytes(b"generated-media-output")
        return 0
    if mode == "ffmpeg-no-output":
        return 0
    if mode == "ffmpeg-fail":
        print("ffmpeg fixture failure", file=sys.stderr)
        return 9
    raise ValueError("unknown fake media tool mode")


if __name__ == "__main__":
    raise SystemExit(main())
