# Milestone 3 verification report

## 1. Summary

- Date/platform: 2026-08-01, Windows, Node 22.23.2, pnpm 9.15.4, Python 3.11.9, Rust/Cargo 1.97.1.
- Milestone 3 was reopened after an independent audit found unwired consent, a phantom transcript artifact, no real fallback, production deterministic ASR, non-operational invalidation, and transcript project-isolation gaps.
- The repaired pipeline now runs `queue -> consent and model verification -> project-scoped worker -> artifact verification/registry -> segment persistence -> review_required`.
- No Milestone 4 translation, TTS, voice cloning, cloud credential, or voice catalog work was added.

## 2. Architecture decisions

- `AsrPipelineService` is the Rust coordinator. It accepts only a verified `OriginalAudio` artifact owned by the job project.
- `WorkerManager.client_for_stage` performs consent and installed-model integrity checks on the same path that creates the worker client. FunASR and faster-whisper both require consent when fallback is configured.
- Installed models are local-only and require `vietdub-model.json`; provider/license must match consent and every declared file is contained, size-checked, and SHA-256 verified.
- Python writes a request-unique transcript artifact below the canonical project working directory. Rust independently checks file existence, containment, hash, size, schema, and segment invariants before registration.
- Full ASR replaces the transcript transactionally. Regional rerun replaces only overlapping segments and preserves unaffected segment IDs/data.
- Review checkpoints are persisted directly as `review_required` completed jobs and do not occupy a queue slot. Approval completes the latest transcript-review stage.
- Production ASR contains only FunASR/faster-whisper adapters and fallback composition. Deterministic behavior exists only in tests/fixtures.

## 3. Files created

- `apps/desktop/src-tauri/src/infrastructure/asr_pipeline.rs`
- `workers/asr/providers/fallback_provider.py`
- `schemas/installed-model-manifest.schema.json`
- `tests/fixtures/asr_workers/asr/main.py`

The obsolete production file `workers/asr/providers/deterministic_provider.py` was removed.

## 4. Files modified

- Rust domain/persistence/orchestration: transcript domain, segment repository, invalidation engine, queue, stage-run repository, worker client/manager, app state, transcript/model commands, module exports, and Milestone 3 tests.
- Python ASR: worker entry point, FunASR/faster-whisper adapters, contract/worker tests, and regional filtering.
- Governance/schema/docs: pipeline architecture, threat model, test strategy, ADR 0006, resource manifest guidance, implementation plan, and this verification report.
- No dependency manifest or lockfile dependency entry changed.

## 5. Commands executed

- `pnpm install --offline --frozen-lockfile --force`
- `pnpm lint`
- `pnpm typecheck`
- `pnpm build`
- `cargo clippy --workspace --all-targets --locked -- -D warnings`
- `pnpm test`
- Focused Rust/Python Milestone 3 tests and schema/security source scans.

The shell environment prepended the existing `.venv/Scripts` and installed Rust toolchain directories because the session PATH initially exposed Windows Store Python and omitted Cargo.

## 6. Build results

- Web TypeScript/Vite production build: passed.
- Rust workspace locked build: passed.
- TypeScript strict typecheck: passed.
- Python strict mypy: passed, 20 source files.
- ESLint, Ruff, `cargo fmt --check`, and Clippy with warnings denied: passed.
- Eight JSON Schemas passed Draft 2020-12 schema validation.

## 7. Test results

- Web: 3 passed.
- Rust: 71 passed (24 unit, 11 Milestone 1, 20 Milestone 3, 11 Milestone 2, 5 worker protocol).
- Python: 42 passed, including 31 ASR contract/worker/segmentation tests.
- Python cross-process integration: 1 passed.
- Total reported test cases: 117 passed, 0 failed.

Milestone 3 coverage includes real artifact creation/hash verification, required primary/fallback consent, installed-model corruption, project isolation, transactional rollback, timestamp/speaker edit, split/merge edge cases, persisted invalidation, regional rerun preservation, and review-slot release.

## 8. Security checks

- Production fake scan: no deterministic ASR provider/default under production `workers/asr`.
- Process scan: no `shell=true`, `eval`, or `exec` launch path.
- Scope scan: no translation/TTS/voice-clone worker directory.
- Audio/output paths are relative, project-contained, and symlink checked in both Rust and Python.
- Worker transcript descriptors cannot become success without a real matching file.
- Models cannot run without consent, a local HTTPS-source manifest, and matching file sizes/SHA-256 hashes.
- No API key, credential, model binary, network call, or new dependency was added.

## 9. Known limitations

- FunASR, faster-whisper, Python runtime, and model binaries are not bundled or downloaded. A release installer/model manager must provision reviewed local resources later.
- Model execution is process-contained and cancellable but not OS-sandboxed; native AI libraries remain a residual risk.
- SQLite and filesystem writes cannot share one atomic transaction. A verified but unregistered worker file may remain after a later database failure and is safe for garbage collection.
- The current desktop shell exposes the backend IPC foundations but does not yet provide the complete production transcript-workspace UI.
- The repository still has no initial Git commit, so historical diff/dependency provenance cannot be independently reconstructed from Git.

## 10. Next milestone

Stop after repaired Milestone 3 acceptance. Milestone 4 (translation) remains unauthorized and has not started.

## 11. Exact continuation prompt

```text
Tiếp tục VietDub Studio từ Milestone 3 đã được xác minh lại.

Trước khi code, kiểm tra repository và chạy lại baseline build/lint/typecheck/test.
Chỉ triển khai Milestone 4 — Translation theo AGENTS.md và docs/roadmap/implementation-plan.md.
Không triển khai TTS, audio separation, composer hoặc release hardening.
Kết thúc bằng báo cáo 11 mục và dừng sau khi Milestone 4 đạt tiêu chí nghiệm thu.
```
