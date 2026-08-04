# Implementation plan

## Baseline assessment (2026-08-01)

Milestone 0 was re-verified before Milestone 1 changes: lint, strict type checks, production builds, and all Web/Rust/Python/integration tests passed. The active toolchain is Node 22.23.2, pnpm 9.15.4, Python 3.11.9, and Rust/Cargo 1.97.1 on Windows.

## Cross-cutting rules

Each milestone stays buildable, adds deterministic tests and documentation, and records acceptance evidence. Business state belongs in Rust/SQLite; binary media stays on disk. All process execution uses fixed executables and argument arrays. Cloud use is opt-in. Schema and migration changes are versioned. No milestone silently expands into out-of-MVP capabilities.

## Milestone 0: repository foundation

**Status:** Implemented and verified on Windows on 2026-08-01. See `milestone-0-verification.md` for acceptance evidence.

**Deliverables:** pnpm/Cargo/Python workspace; Tauri 2 + strict React/Vite UI; versioned JSON Schemas; SQLite migration runner; deterministic echo worker; secure Rust worker client; architecture, threat, licensing, and test documents; Windows CI.

**Acceptance:** desktop dev command starts; frontend typecheck/build pass; Rust workspace builds/tests; Python lint/type/tests pass; Rust receives progress and a terminal echo result; timeout, cancel, safe failure, output limit, and schema rejection are tested; no shell or secret exists.

**Files:** root governance/build files, `apps/desktop/**`, `workers/common/**`, `workers/echo/**`, `schemas/**`, `docs/**`, `tests/**`, and `.github/workflows/ci.yml`.

**Risks:** toolchain availability; native Tauri prerequisites; Windows descendant-process cleanup; drift between Rust/Python schema implementations. Mitigate with clean CI, exact locks, contract tests, and a future Windows Job Object process supervisor.

## Milestone 1: project, artifact, and job foundation

**Status:** Implemented and verified on Windows on 2026-08-01. See `milestone-1-verification.md` for acceptance evidence. No Milestone 2 work has started.

Implement project CRUD and directory creation, relative-path containment, artifact registry and hashes, full StageRun/Job records, persistent priority queue, pause/resume/cancel/retry transitions, recovery of interrupted jobs, cache identity, dependency invalidation, and a test-only deterministic stage provider.

Acceptance includes migration upgrade tests, concurrent-project cancellation isolation, restart recovery, artifact corruption detection, targeted segment invalidation, and no occupied resource at review checkpoints.

## Milestone 2: media import

**Status:** Implemented and verified on Windows on 2026-08-01. See `milestone-2-verification.md` for acceptance evidence. No Milestone 3 work has started.

Add immutable local import first, then HTTPS URL policy and per-site downloader adapter contracts for Douyin, Bilibili, YouTube, and TikTok. Add ffprobe metadata, proxy generation, normalized audio extraction, bounded process supervision, remote filename replacement, redirect/IP validation, and short licensed/generated fixtures.

Acceptance includes shell-metacharacter filenames, traversal/SSRF/redirect/size/timeout tests, local import independence from downloader health, and checked ffprobe/FFmpeg exit status.

## Milestone 3: ASR and transcript

**Status:** Repaired and re-verified on Windows on 2026-08-01 after an independent acceptance audit. See `milestone-3-verification.md` for current evidence. No Milestone 4 work has started.

Add model consent/manifest foundations, worker manager, normalized ASR provider contract, FunASR adapter with faster-whisper fallback, mono ASR audio, subtitle segmentation/QC, transcript review checkpoint, and editor operations including regional rerun.

Acceptance includes adapter contracts without model downloads in default tests, overlap/empty/long/repetition/silence/low-confidence warnings, split/merge timestamp invariants, and correct invalidation after transcript edits.

## Milestone 4: translation

**Status:** Implemented and verified on Windows on 2026-08-01. See `milestone-4-verification.md` for acceptance evidence. No Milestone 5 work has started.

Add credential-store boundary, provider-neutral translation contract, OpenAI-compatible adapter, local-adapter contract, block/context/glossary/proper-name processing, strict structured-response validation, bounded retry, partial-block persistence, and translation review UI.

Acceptance covers missing/duplicate/empty IDs, prose outside schema, locked names, provider failure recovery, cloud disclosure, and targeted invalidation.

## Milestone 5: TTS

**Status:** Implemented and verified on Windows on 2026-08-01. See `milestone-5-verification.md` for acceptance evidence. No Milestone 6 work has started.

Add provider-neutral voice catalog/TTS contract, at least one approved system or cloud voice, global/speaker/segment assignment, previews, per-segment cache, duration measurement and fitting policy, retry, and human warnings above safe stretch thresholds.

Acceptance verifies cache identity, two-speaker routing, single-segment failure isolation, shortening requests, audio metadata, and no voice cloning.

## Milestone 6: audio

**Status:** Implemented and verified on Windows on 2026-08-02. See `milestone-6-verification.md` for acceptance evidence. No Milestone 7 work has started.

Add separation contract and one approved engine, explicit consent/install metadata, fallback attenuation, timeline assembly, background/voice/music controls, ducking, fades, loudness normalization, limiter, clipping and duration QC, and mixed WAV export.

Acceptance covers separation fallback, timeline alignment, no clipping, deterministic filter construction, and preservation of successful TTS artifacts.

## Milestone 7: composer

**Status:** Implemented and verified on Windows on 2026-08-02. See `milestone-7-verification.md` for acceptance evidence. No Milestone 8 work has started.

Add validated subtitle rendering, trim/crop/resize/aspect/padding/blur/flip/speed, text/logo/watermark/cover regions, preview presets, MP4 rendering, SRT/WAV export, output probing, and quality checks. Do not expose raw FFmpeg options.

Acceptance verifies filter escaping, bounds, aspect presets, soft/burned subtitles, source immutability, render cancellation/retry, and output streams/duration.

## Milestone 8: hardening and release

**Status:** Implementation and local Windows verification complete on 2026-08-02. See `milestone-8-verification.md`. Final acceptance remains pending the protected GitHub release run because this repository context has no trusted Authenticode certificate; no signed artifact or separate clean-runner evidence has been fabricated.

Finish model manager and verified manifests, OS credential storage, privacy/log rotation, crash/process-tree recovery, dependency and license audit, SBOM/notices, Windows installer, signed update design, profiling, complete deterministic E2E/security suites, and release checklist.

Acceptance requires clean-machine installation, no silent downloads/updates, checksum failure refusal, credential/log inspection, SBOM reconciliation, crash recovery, performance budgets, and signed release artifacts.

## Next authorized step

There is no Milestone 9 in this plan. Do not add product scope. The only continuation is Milestone 8 release acceptance: configure protected Authenticode secrets, run `.github/workflows/release.yml`, and record successful signature, manifest, and separate clean-machine install/uninstall evidence in `milestone-8-verification.md`. Until that external gate passes, Milestone 8 must not be represented as fully accepted.
