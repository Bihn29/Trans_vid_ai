# Local Vietnamese dub render acceptance

## Acceptance criteria

- A translated project can synthesize Vietnamese speech locally without cloud consent.
- MeloTTS 44.1 kHz clips can be fitted to the normalized 16 kHz project timeline.
- A failed downstream mix or render can reuse verified per-segment TTS and background artifacts.
- The render keeps the full source frame, blurs the lower hard-subtitle band, burns Vietnamese subtitles over that band, and mixes Vietnamese speech with the retained background.
- A completed render can be exported again after subtitle or layout adjustments.

## Verified sample

- Source duration: 996.1 seconds.
- Output: H.264 1920x1080 with mono AAC audio at 16 kHz.
- Output duration: 996.1 seconds.
- Output size: 489,289,742 bytes.
- Full Rust workspace tests, web lint/build/tests, Python Ruff/mypy, and 71 Python worker tests passed on 2026-08-02.

