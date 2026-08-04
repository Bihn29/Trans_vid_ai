# 0010. Typed composer and contained render plan

- **Status:** Approved
- **Deciders:** Antigravity Team
- **Date:** 2026-08-02

## Context

Milestone 7 needs subtitle composition, video transforms, overlays, preview presets, MP4 rendering, export, cancellation, and output quality checks. FFmpeg filter graphs are expressive enough to read files or interpret punctuation, so exposing a raw option or interpolating untrusted text would cross the existing UI-to-Rust and Rust-to-process security boundaries.

## Decision

1. Rust owns a versioned `ComposerConfig` with bounded enums, dimensions, timing, colors, opacity, speed, and overlay counts. There is no raw FFmpeg field.
2. Rust deterministically builds an argument vector and filter graph. The shell is never used. Source, mixed audio, subtitle, overlay, temporary, and output paths must pass `ProjectRelativePath` validation and execute with the canonical project root as working directory.
3. User subtitle and text content is written to contained data files. Filters reference generated ASCII relative filenames; user content is never embedded in a filter expression. Logo/watermark inputs must be verified same-project `overlay_image` artifacts.
4. Render identity hashes the full typed config plus verified source, mixed-audio, overlay-image hashes and enabled subtitle dependency hashes. Composer changes use the existing dependency invalidation engine.
5. FFmpeg writes a request-unique partial MP4. Rust requires a successful supervised exit and non-empty regular file before promotion. ffprobe then independently verifies video/audio presence, exact target dimensions, and duration tolerance before render and QC artifacts are registered.
6. Job cancellation uses the existing per-job token and removes partial or race-completed composer outputs before acknowledgement. Retry creates a new Job/StageRun attempt and output path.
7. SRT and mixed WAV export accepts only verified artifacts, an absolute destination with the matching fixed extension, a canonical non-symlink parent, and create-new semantics. Existing files are never overwritten.

## Consequences

- The architecture remains Rust-owned and specializes the guarded media-tool boundary; no existing decision is replaced.
- No dependency, binary, codec, cloud service, or silent download is added. Production rendering remains fail-closed until separately reviewed FFmpeg/ffprobe manifests and binaries are configured.
- Filter plan tests do not require codec binaries. End-to-end orchestration tests use only the deterministic fake media tool under `tests/fixtures`.
- Rich font selection and advanced subtitle styling are deferred; their future design must preserve the same typed/file-backed boundary.
