# Milestone 6 verification report

## 1. Summary

- Date/platform: 2026-08-02, Windows, Node 22.23.2, pnpm 9.15.4, Python 3.11.9, Rust/Cargo 1.97.1.
- Baseline Milestone 5 was rechecked before code: lint, strict typecheck, production build, and all 156 existing tests passed.
- Milestone 6 now provides a provider-neutral local separation contract, approved built-in energy-mask engine metadata, explicit attenuation fallback, persisted mix controls, typed deterministic timeline assembly, TTS fitting, ducking/fades, bounded RMS normalization, peak limiting, audio QC, and registered mixed PCM WAV export.
- Final lint, typecheck, build, Clippy, schema validation, and all 168 tests pass.
- No Milestone 7 subtitle composition, video filtering/rendering, raw FFmpeg option, or release-hardening work was added.

## 2. Architecture decisions

- ADR 0009 records the separation/provider boundary and typed Rust DSP plan. It specializes the existing worker/artifact/job architecture without replacing it.
- Demucs was reviewed but rejected for this milestone: the official repository declares its code MIT, while upstream pretrained-weight licensing remains unclear. No Demucs package, code, weights, download, or model consent was added.
- The approved `energy-mask-v1` engine is clean-room bundled source using existing Python/runtime facilities only. Its manifest explicitly declares no install, silent download, model, consent, credential, or off-device transfer.
- Rust owns fallback, timeline construction, mixing, normalization, limiting, QC, and output registration. No user-controlled FFmpeg filter or shell string is constructed.
- Mixing reads verified TTS artifacts as immutable inputs and writes only a new request-unique `audio/mixed` artifact.

## 3. Files created

- Rust: `0006_milestone_six.sql`, audio domain/repository/pipeline/commands, and `tests/milestone_six.rs`.
- Python: provider-neutral separation contract, built-in energy-mask worker, package exports, and four unit/security tests.
- Schemas/manifests/fixtures: separation request schema, audio-mix schema, approved engine manifest, and a test-only failing separation fixture.
- Web: audio types, `AudioMixer` component, and two UI tests.
- Governance: ADR 0009 and this verification report.

## 4. Files modified

- Rust module exports, migration registry, ArtifactKind, project directory layout, WorkerManager routing/timeout, application state, Tauri registration, invalidation engine, and the previous scope guard.
- Web application shell, styles, and Vietnamese audio labels.
- Pipeline architecture, threat model, test strategy, dependency/license policy, roadmap status, and next-authorized-step guard.
- No dependency manifest or lockfile dependency entry changed.

## 5. Commands executed

- Baseline and final: `pnpm lint`, `pnpm typecheck`, `pnpm build`, and `pnpm test`.
- Rust: focused `cargo test --test milestone_six`, `cargo fmt --all -- --check`, and `cargo clippy --workspace --all-targets --locked -- -D warnings`.
- Python: focused Ruff, strict mypy, and separation pytest runs.
- Web: focused ESLint, TypeScript typecheck, and Vitest runs.
- Draft 2020-12 validation for all 14 JSON Schemas.
- Source scans for network/process use in separation, raw media filters, composer/render scope, custom/clone voice inputs, and external model/package references.

The shell prepended the existing `.venv/Scripts` and installed Rust toolchain directories because the session PATH exposes Windows Store Python and omits Cargo.

## 6. Build results

- Web TypeScript/Vite production build: passed.
- Rust workspace locked build: passed.
- TypeScript strict typecheck and Python strict mypy across 42 source files: passed.
- ESLint, Ruff, `cargo fmt --check`, and Clippy with warnings denied: passed.
- All 14 versioned JSON Schemas passed Draft 2020-12 schema checks.

## 7. Test results

- Web: 9 passed.
- Rust: 91 passed (25 unit, 6 Milestone 6, 7 Milestone 5, 6 Milestone 4, 11 Milestone 1, 20 Milestone 3, 11 Milestone 2, 5 worker protocol).
- Python: 67 passed, including 4 separation contract/security tests.
- Python cross-process integration: 1 passed.
- Total reported test cases: 168 passed, 0 failed.

Milestone 6 coverage includes aligned verified stems, reconstructable deterministic separation, explicit provider-failure attenuation, deterministic timeline/output hashes, exact output duration, bounded settings persistence, duck/fade/gain paths, limiting with zero post-limit clipping, cancellation cleanup, project layout isolation, and unchanged verified TTS artifacts.

## 8. Security checks

- Separation production source contains no network/process API, cloud endpoint, credential, model import, silent download, or third-party engine package.
- Input/output paths are project-relative and reject traversal/symlinks. Python bounds source size/format/frames; Rust independently verifies SHA-256, size, PCM format, ownership, kind, and stem alignment.
- The mixer accepts only same-project registered artifacts and bounded PCM16 mono WAV. Gain, ducking, fade, normalization, limiter, sample rate, file size, and timeline arithmetic are range-checked.
- Output uses a create-new temporary file, flush/sync, contained rename, independent registry hash/size, and cleanup on registration failure.
- Cancellation is acknowledged through the per-job token, writes no mixed artifact, and preserves every verified TTS artifact.
- A failed/invalid/unavailable worker triggers only the clearly labeled local attenuation fallback; cancellation never becomes success.
- No dependency was added. Demucs/model assets were rejected rather than included under unclear weight licensing.

## 9. Known limitations

- `energy-mask-v1` is a deterministic baseline, not a neural-quality source separator. It separates mono PCM by frame energy and can leak voice/music; the UI exposes when fallback attenuation is used.
- DSP currently supports bounded mono PCM16 WAV from 8–48 kHz and loads each bounded input into memory. The 512 MiB cap limits but does not eliminate memory pressure.
- Loudness normalization is RMS dBFS, not gated LUFS/EBU R128 measurement. The peak limiter is deterministic hard limiting, not a look-ahead mastering limiter.
- A music gain lane is supported when a verified music/background artifact is supplied, but the built-in separation engine produces only vocals and background stems.
- Tauri commands expose engine metadata and persisted mix settings; the current shell still lacks complete end-to-end scheduler/start controls.
- The repository still has no initial Git commit; historical diff and dependency provenance cannot be independently reconstructed from Git.

## 10. Next milestone

Stop after Milestone 6 acceptance. Milestone 7 (subtitle composition, video filtering, and rendering) has not started and requires explicit authorization.

## 11. Exact continuation prompt

```text
Tiếp tục VietDub Studio từ Milestone 6 đã được xác minh.

Trước khi code, kiểm tra repository và chạy lại baseline build/lint/typecheck/test.
Chỉ triển khai Milestone 7 — composer theo AGENTS.md và docs/roadmap/implementation-plan.md.
Không triển khai release hardening.
Không thêm dependency trước khi kiểm tra license và security.
Kết thúc bằng báo cáo 11 mục và dừng sau khi Milestone 7 đạt tiêu chí nghiệm thu.
```
