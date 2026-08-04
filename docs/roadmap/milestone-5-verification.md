# Milestone 5 verification report

## 1. Summary

- Date/platform: 2026-08-01, Windows, Node 22.23.2, pnpm 9.15.4, Python 3.11.9, Rust/Cargo 1.97.1.
- Baseline Milestone 4 was rechecked before code: lint, strict typecheck, production build, and all 140 existing tests passed.
- Milestone 5 now provides a provider-neutral voice catalog/TTS contract, approved OpenAI-compatible cloud voices, project/speaker/segment assignment, preview artifacts, durable per-segment execution/cache/recovery, verified PCM WAV metadata, duration-fitting warnings, and UI disclosure.
- Final lint, typecheck, build, Clippy, schema validation, and all 156 tests pass.
- No Milestone 6 separation, time-stretch implementation, mixing, composer, custom voice, or voice-cloning work was added.

## 2. Architecture decisions

- ADR 0008 records the provider-neutral TTS, credential, cache, preview, and duration-fitting boundary; it specializes the existing Rust/SQLite plus Python-worker architecture without replacing it.
- Rust owns assignment precedence and durable per-segment state. Cache identity includes translated-text hash, provider, voice, model, and requested speed.
- Python implements the neutral provider contract. The approved adapter uses a fixed HTTPS speech request and PCM WAV response with no cloud SDK dependency. The supported request shape and WAV response format were checked against the official OpenAI audio API documentation.
- Provider output is untrusted. Python validates WAV before writing; Rust independently verifies containment, descriptor type, SHA-256, size, PCM structure, sample metadata, and measured duration before persistence.
- Duration fitting records a proposed playback rate clamped to 0.85–1.20. Unsafe values request a shorter translation or warn about excessive slowdown; Milestone 5 does not alter audio samples.
- Credentials remain behind `CredentialStore`; the production default fails closed until OS credential-store work in Milestone 8.

## 3. Files created

- Rust: `0005_milestone_five.sql`, `domain/tts.rs`, `persistence/tts.rs`, `infrastructure/tts_pipeline.rs`, `commands/tts.rs`, and `tests/milestone_five.rs`.
- Python: `workers/tts/contract.py`, the OpenAI-compatible adapter, JSONL worker entry point, package exports, and six contract/security tests.
- Schemas/fixtures: `tts-request.schema.json`, `voice-catalog.schema.json`, approved provider manifest, and deterministic test-only WAV worker fixture.
- Web: TTS types, `VoiceStudio` component, and two UI tests.
- Governance: ADR 0008 and this verification report.

## 4. Files modified

- Rust module exports, migration registry, worker-stage routing, application state, command registration, invalidation matching, and the previous scope guard.
- Web application shell, styles, and Vietnamese i18n milestone/voice labels.
- Pipeline architecture, threat model, test strategy, roadmap status, and next-authorized-step guard.
- No dependency manifest or lockfile dependency entry changed.

## 5. Commands executed

- Baseline and final: `pnpm lint`, `pnpm typecheck`, `pnpm build`, and `pnpm test`.
- Rust: `cargo test --test milestone_five`, `cargo fmt --all -- --check`, and `cargo clippy --workspace --all-targets --locked -- -D warnings`.
- Python: focused Ruff, mypy, and TTS pytest runs.
- Web: focused ESLint, TypeScript typecheck, and Vitest runs.
- Draft 2020-12 validation for all 12 JSON Schemas.
- Source scans for custom/clone voice inputs, persisted credential-like fields, production fakes, and out-of-scope worker directories.

The shell prepended the existing `.venv/Scripts` and installed Rust toolchain directories because the session PATH exposes Windows Store Python and omits Cargo.

## 6. Build results

- Web TypeScript/Vite production build: passed.
- Rust workspace locked build: passed.
- TypeScript strict typecheck and Python strict mypy across 37 source files: passed.
- ESLint, Ruff, `cargo fmt --check`, and Clippy with warnings denied: passed.
- All 12 versioned JSON Schemas passed Draft 2020-12 schema checks.

## 7. Test results

- Web: 7 passed.
- Rust: 85 passed (25 unit, 7 Milestone 5, 6 Milestone 4, 11 Milestone 1, 20 Milestone 3, 11 Milestone 2, 5 worker protocol).
- Python: 63 passed, including 7 TTS contract/provider tests.
- Python cross-process integration: 1 passed.
- Total reported test cases: 156 passed, 0 failed.

Milestone 5 coverage includes deterministic cache identity, corrupt-cache regeneration, two-speaker routing, segment override/project isolation, preview isolation, single-segment failure isolation, retry reuse, restart recovery, PCM audio metadata, shortening/slowdown warnings, HTTPS/redirect/IP/size controls, and rejection of custom voices.

## 8. Security checks

- Voice requests contain only an approved catalog ID; schemas and production sources contain no custom sample, speaker embedding, reference audio, or voice-cloning field.
- Cloud execution requires explicit off-device disclosure and consent. The adapter enforces HTTPS, no credentials/fragments/non-default ports, no redirects, non-global literal-IP rejection, bounded retries, and a 32 MiB response cap.
- The TTS secret exists only in the injected Rust test credential store. Scans found no secret value in production sources, migrations, provider manifests, project artifacts, or database fields.
- Worker files must be request-unique, project-relative, non-symlink paths. Rust recomputes SHA-256 and size and parses PCM WAV before registry insertion; corrupt cache audio is refused and regenerated.
- Assignment commands enforce project ownership and invalidate affected TTS/project-tail stage runs without racing running work.
- The deterministic TTS provider exists only in `tests/fixtures/tts_workers`; production has only the provider contract and approved network adapter.
- No dependency was added, so there is no new package-license or package-advisory surface. The remote provider's off-device behavior and service terms are declared in its manifest.

## 9. Known limitations

- The OS keychain implementation is intentionally deferred to Milestone 8. Production cloud execution uses `UnavailableCredentialStore` and therefore fails closed; tests inject a non-persisting credential implementation.
- Milestone 5 records a duration-fitting proposal and warnings but does not time-stretch, separate, align, mix, normalize, or export audio. Those operations are Milestone 6.
- The Tauri catalog/assignment/preview commands and UI component exist, but the project shell does not yet provide complete end-to-end scheduler controls or production credential configuration.
- Only the approved `alloy` and `nova` catalog subset is exposed. Additional voices/providers require manifest, disclosure, contract, license, and security review.
- SQLite and the filesystem cannot share one atomic transaction. A valid worker file may remain if a later database operation fails; registry insertion is compensated where possible and remaining files are eligible for later garbage collection.
- The repository still has no initial Git commit; historical diff and dependency provenance cannot be independently reconstructed from Git.

## 10. Next milestone

Stop after Milestone 5 acceptance. Milestone 6 (audio separation and mixing) has not started and requires explicit authorization.

## 11. Exact continuation prompt

```text
Tiếp tục VietDub Studio từ Milestone 5 đã được xác minh.

Trước khi code, kiểm tra repository và chạy lại baseline build/lint/typecheck/test.
Chỉ triển khai Milestone 6 — audio theo AGENTS.md và docs/roadmap/implementation-plan.md.
Không triển khai composer hoặc release hardening.
Không thêm dependency trước khi kiểm tra license và security.
Kết thúc bằng báo cáo 11 mục và dừng sau khi Milestone 6 đạt tiêu chí nghiệm thu.
```
