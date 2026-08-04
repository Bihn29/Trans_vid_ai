# Milestone 4 verification report

## 1. Summary

- Date/platform: 2026-08-01, Windows, Node 22.23.2, pnpm 9.15.4, Python 3.11.9, Rust/Cargo 1.97.1.
- Baseline Milestone 3 was rechecked before code: lint, strict typecheck, production build, and all 117 existing tests passed.
- Milestone 4 now provides a credential-store boundary, provider-neutral translation worker, OpenAI-compatible and local adapter contracts, persistent partial blocks, strict two-sided validation, bounded retry/recovery, cloud disclosure, and translation review UI.
- No Milestone 5 TTS, voice catalog, audio separation, composer, or release-hardening work was added.

## 2. Architecture decisions

- ADR 0007 records the translation/provider/credential boundary; it specializes the existing Rust/SQLite plus Python-worker architecture rather than replacing it.
- Rust owns block identity and persistence. Identity includes source hashes plus provider/model/language/glossary/locked-name inputs, so completed blocks are reused only when all relevant dependencies match.
- Credentials are retrieved as redacted `SecretString` values immediately before execution and travel only through the bounded worker IPC request. The secure production default refuses cloud execution until an OS credential-store adapter exists.
- Python validates provider output before writing a result artifact. Rust then verifies project containment, SHA-256, size, schema, exact IDs, and locked names before committing a block and its segment translations atomically.
- A translation review checkpoint is durable but releases its queue slot, matching the existing human-review architecture.

## 3. Files created

- Rust: `0004_milestone_four.sql`, translation domain/repository/pipeline, credential boundary, translation commands, and `milestone_four.rs` tests.
- Python: `workers/translation/**` provider contracts, adapters, entry point, and 14 tests.
- Schemas/fixtures: translation request/result schemas and the deterministic test-only translation worker fixture.
- Web: translation types, `TranslationReview` component, and UI tests.
- Governance: ADR 0007 and this verification report.

## 4. Files modified

- Rust module exports, migration registry, artifact kind, worker manager, persistent review queue, transcript invalidation behavior, application state, Tauri command registration, and the Milestone 3 scope guard.
- Web application shell, styles, and milestone label.
- Roadmap implementation status and next authorized step.
- No dependency manifest or lockfile dependency entry changed.

## 5. Commands executed

- Baseline and final: `pnpm lint`, `pnpm typecheck`, `pnpm build`, and `pnpm test`.
- Rust: focused `cargo check`, `cargo test --test milestone_four`, `cargo fmt --all`, and `cargo clippy --workspace --all-targets --locked -- -D warnings`.
- Python: focused Ruff, strict mypy, and translation pytest runs.
- Web: focused ESLint, TypeScript typecheck, and Vitest runs.
- Security/source scans for secrets, production fakes, out-of-scope worker directories, unsafe process APIs, and dependency-manifest changes.

The shell prepended the existing `.venv/Scripts` and installed Rust toolchain directories because the session PATH exposes Windows Store Python and omits Cargo.

## 6. Build results

- Web TypeScript/Vite production build: passed.
- Rust workspace locked build: passed.
- TypeScript strict typecheck and Python strict mypy: passed.
- ESLint, Ruff, `cargo fmt --check`, and Clippy with warnings denied: passed.
- All versioned JSON Schemas passed Draft 2020-12 schema checks.

## 7. Test results

- Web: 5 passed.
- Rust: 78 passed (25 unit, 6 Milestone 4, 11 Milestone 1, 20 Milestone 3, 11 Milestone 2, 5 worker protocol).
- Python: 56 passed, including 14 translation contract/provider/worker tests.
- Python cross-process integration: 1 passed.
- Total reported test cases: 140 passed, 0 failed.

Milestone 4 coverage includes prose outside JSON, missing/duplicate/empty IDs, locked-name preservation, prompt context/glossary, bounded retry, partial-block reuse, restart recovery, cloud disclosure, secret non-persistence, cancellation isolation, artifact verification, review-slot release, and segment-targeted downstream invalidation.

## 8. Security checks

- Credential values have no SQLite column or project-artifact field; tests inspect the database and project files for the injected secret.
- `SecretString` diagnostics are always redacted. Worker failures expose only stable safe error codes/messages.
- Cloud execution requires an off-device disclosure, explicit consent, endpoint, credential reference, and a retrievable credential.
- The OpenAI-compatible adapter accepts HTTPS only, rejects credentials/fragments/non-443 ports/non-global literal IPs, disables redirects, bounds response size, and never returns its API key in metrics.
- Provider output is untrusted and must pass Python and Rust validation. Artifacts are contained, SHA-256/size checked, and registered before job success.
- The deterministic provider is present only in test files/fixtures. No TTS or voice-cloning production worker exists.
- No new dependency was added, so no new license or advisory surface was introduced.

## 9. Known limitations

- The OS keychain implementation is intentionally deferred to release hardening; the production `UnavailableCredentialStore` fails closed. Tests inject a credential-store implementation without persisting the value.
- The local translation adapter contract exists, but no concrete local model/runtime is bundled or downloaded.
- The review component and Tauri commands are present, but the current project shell still lacks complete end-to-end screen routing and scheduler controls.
- SQLite and filesystem writes cannot form one transaction. A verified but unregistered worker file can remain after a later database failure and is eligible for garbage collection.
- The repository still has no initial Git commit; historical diff and dependency provenance cannot be independently reconstructed from Git.

## 10. Next milestone

Stop after Milestone 4 acceptance. Milestone 5 (TTS) has not started and requires explicit authorization.

## 11. Exact continuation prompt

```text
Tiếp tục VietDub Studio từ Milestone 4 đã được xác minh.

Trước khi code, kiểm tra repository và chạy lại baseline build/lint/typecheck/test.
Chỉ triển khai Milestone 5 — TTS theo AGENTS.md và docs/roadmap/implementation-plan.md.
Không triển khai audio separation, composer hoặc release hardening.
Không thêm dependency trước khi kiểm tra license và security.
Kết thúc bằng báo cáo 11 mục và dừng sau khi Milestone 5 đạt tiêu chí nghiệm thu.
```
