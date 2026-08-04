# Milestone 7 verification report

## 1. Summary

- Date/platform: 2026-08-02, Windows, Node 22.23.2, pnpm 9.15.4, Python 3.11.9, Rust/Cargo 1.97.1.
- Baseline Milestone 6 was rechecked before code: lint, strict typecheck, production build, and all 168 existing tests passed.
- Milestone 7 now provides persisted typed composer settings, bounded overlay import, deterministic SRT generation and render identity, trim/crop/resize/aspect/padding/blur/flip/speed filters, text/logo/watermark/cover regions, soft/burned subtitles, draft/final presets, supervised MP4 render, independent ffprobe QC, and verified SRT/WAV export.
- Final lint, typecheck, build, Clippy, schema validation, and all 177 tests pass.
- No Milestone 8 release-hardening, installer, updater, SBOM, credential-store implementation, custom voice input, or voice cloning was added.

## 2. Architecture decisions

- ADR 0010 records the typed composer and contained render-plan boundary. It specializes the approved Rust/process/artifact architecture without replacing it.
- Rust owns all composer validation, dependency identity, filter construction, output promotion, probing, QC, and registration. There is no raw FFmpeg option in domain, schema, IPC, or UI.
- Subtitle and user text content is written to contained generated data files. Only restricted ASCII relative paths enter filter syntax; user text cannot become a filter fragment.
- Logo/watermark inputs are bounded generated `overlay_image` artifacts. All source, mixed-audio, subtitle, image, output, and QC paths remain project-contained.
- Existing PersistentQueue and StageRun/Job state machines own cancellation, retry, recovery, and history. A cancellation removes partial or race-completed composer outputs before acknowledgement.

## 3. Files created

- Rust persistence/domain/runtime: `0007_milestone_seven.sql`, composer domain and repository, composer pipeline/asset/export services, composer commands, and `tests/milestone_seven.rs`.
- Schema: `schemas/composer-config.schema.json`.
- Web: composer types, `ComposerPanel`, and two UI tests.
- Governance: ADR 0010 and this verification report.

## 4. Files modified

- Rust module exports, migration registry/tests, ArtifactKind, application state, Tauri command registration, and the composer timing/QC boundary.
- Web application shell, styles, and Vietnamese labels/milestone marker.
- Pipeline architecture, threat model, test strategy, roadmap status, and next-authorized-step guard.
- No dependency manifest or lockfile dependency entry changed.

## 5. Commands executed

- Baseline and final: `pnpm lint`, `pnpm typecheck`, `pnpm build`, and `pnpm test`.
- Rust: focused `cargo test --test milestone_seven --locked`, `cargo fmt --all -- --check`, and `cargo clippy --workspace --all-targets --locked -- -D warnings`.
- Web: focused strict TypeScript typecheck and Vitest run.
- Draft 2020-12 validation for all 15 JSON Schemas.
- Source scans for shell/process/network calls, raw FFmpeg input, uncontained paths, and Milestone 8 scope.

The shell prepended the existing `.venv/Scripts` and installed Rust toolchain directories because the session PATH exposes Windows Store Python and omits Cargo.

## 6. Build results

- Web TypeScript/Vite production build: passed.
- Rust workspace locked build: passed.
- TypeScript strict typecheck and Python strict mypy across 42 source files: passed.
- ESLint, Ruff, `cargo fmt --check`, and Clippy with warnings denied: passed.
- All 15 versioned JSON Schemas passed Draft 2020-12 schema checks.

## 7. Test results

- Web: 11 passed.
- Rust: 98 passed (25 unit, 7 Milestone 7, 6 Milestone 6, 7 Milestone 5, 6 Milestone 4, 11 Milestone 1, 20 Milestone 3, 11 Milestone 2, 5 worker protocol).
- Python: 67 passed.
- Python cross-process integration: 1 passed.
- Total reported test cases: 177 passed, 0 failed.

Milestone 7 coverage includes filter-content isolation, path validation, crop/overlay bounds, all aspect presets, soft/burned subtitle plans, deterministic dependency identity, invalidation, bounded overlay import, source immutability, verified render/SRT/QC artifacts, video/audio/duration/dimension checks, cancellation cleanup, retry completion, corrupt export rejection, and no-overwrite export.

## 8. Security checks

- Composer production code contains no shell, direct process creation, network API, cloud endpoint, credential, model, silent download, or user-controlled raw media option.
- Every plan path passes `ProjectRelativePath`; the supervised process runs under the canonical project root. Artifact inputs must match project, kind, SHA-256, size, regular-file, and non-symlink checks.
- Text/subtitle content is file-backed. Colors, numbers, enums, counts, timings, opacity, speed, crop, dimensions, and regions are bounded before plan construction.
- Overlay import accepts only absolute non-symlink regular PNG/JPEG/WebP paths up to 32 MiB, copies to a generated contained path, flushes, and registers an independently hashed artifact.
- Render uses a checksum-approved executable, discrete arguments, bounded output, timeout/cancellation, create-new partial output, contained rename, ffprobe QC, and cleanup on every checked failure path.
- External SRT/WAV export verifies integrity again, requires a matching extension and canonical non-symlink parent, and uses create-new so existing data is never overwritten.
- No dependency was added, so no new license or supply-chain approval was required.

## 9. Known limitations

- VietDub Studio still does not bundle or silently download FFmpeg/ffprobe. Production rendering fails closed until separately reviewed binary manifests/checksums and installation/configuration are supplied; tests use only the deterministic test fixture.
- Tauri currently exposes composer config, overlay import, and SRT/WAV export. Full UI-to-queue render-start orchestration remains unavailable until approved media tools can be configured; the Rust render/queue service and acceptance contract are implemented and tested.
- Overlay import validates containment, extension, and size but deliberately does not decode the image. A malformed image is rejected by the supervised FFmpeg render, not during import.
- Text rendering uses FFmpeg's configured default font; font catalog/embedding and advanced subtitle styling are not included.
- ffprobe QC checks streams, duration, and dimensions, but visual quality remains a human review concern.
- The repository still has no initial Git commit; historical diff and dependency provenance cannot be independently reconstructed from Git.

## 10. Next milestone

Stop after Milestone 7 acceptance. Milestone 8 (hardening and release) has not started and requires explicit authorization. By roadmap milestone count, 7 of 8 milestones are verified (87.5%); this is not an effort estimate for release hardening.

## 11. Exact continuation prompt

```text
Tiếp tục VietDub Studio từ Milestone 7 đã được xác minh.

Trước khi code, kiểm tra repository và chạy lại baseline build/lint/typecheck/test.
Chỉ triển khai Milestone 8 — hardening và release theo AGENTS.md và docs/roadmap/implementation-plan.md.
Không mở rộng sang voice cloning, lip-sync, OCR, face swap, cloud rendering hoặc plugin marketplace.
Không thêm dependency trước khi kiểm tra license và security.
Kết thúc bằng báo cáo 11 mục và chỉ dừng khi Milestone 8 đạt tiêu chí nghiệm thu.
```
