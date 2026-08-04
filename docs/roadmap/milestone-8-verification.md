# Milestone 8 implementation and verification report

## 1. Summary

- Date/platform: 2026-08-02, Windows, Node 22.23.2, pnpm 9.15.4, Python 3.11.9, Rust/Cargo 1.97.1.
- Milestone 7 was rechecked before code: lint, strict typecheck, production build, and all 177 existing tests passed.
- Milestone 8 implements verified model catalogs/installations, native Windows credentials, bounded privacy logs, runtime crash recovery, deterministic Windows-target SBOM/notices, advisory gates, NSIS packaging, Authenticode/release-manifest policy, performance budgets, and deterministic security/E2E coverage.
- Final local lint, typecheck, build, Clippy, 17 schemas, release audit, all 190 tests, release build, NSIS install/start/uninstall smoke, npm high/critical audit, and scoped RustSec audit pass.
- Milestone 8 is code-complete but **not fully accepted**: a trusted Authenticode certificate is not available in this repository context, so the protected signing workflow and its separate clean Windows runner have not executed. The unsigned local installer is verification evidence only and is not a release artifact.
- No FFmpeg, ffprobe, yt-dlp, model weight, cloud SDK, updater, WebView2 bootstrapper, custom voice, voice cloning, or new product milestone was added.

## 2. Architecture decisions

- ADR 0011 records the model, credential, privacy, recovery, signature, installer, and SBOM trust boundaries. It specializes the approved Rust-owned architecture without replacing provider or pipeline contracts.
- Models are explicit user-provided installations. The bundled catalog is metadata only; catalog approval, consent, exact identity, contained relative paths, size, and SHA-256 must all pass before worker creation.
- Secrets exist only in Windows Credential Manager. SQLite, project files, logs, diagnostics, IPC responses, fixtures, and environment fallbacks do not store them.
- Runtime sessions persist crash state; startup invokes existing queue recovery and removes only contained dot-prefixed partial files. Existing Windows Job Objects remain the descendant process-tree control.
- The application contains no updater. NSIS uses current-user/zlib and skips WebView2 installation. Tauri signs the patched application and installer from a protected ephemeral certificate store; a release manifest then binds exact filename, SHA-256, size, channel, and version.

## 3. Files created

- Rust: migration `0008_milestone_eight.sql`; hardening privacy/recovery/release modules; `infrastructure/model_manager.rs`; `commands/hardening.rs`; and `tests/milestone_eight.rs`.
- Contracts/resources: approved-model and release-manifest schemas; Faster Whisper and blocked FunASR catalog manifests.
- Release tooling/artifacts: `release_audit.py`, `build_release_manifest.py`, `validate_schemas.py`, deterministic CycloneDX SBOM, third-party notices, performance budgets, signed-update design, release checklist, and security exceptions.
- Verification/governance: deterministic release-hardening integration tests, ADR 0011, signed release workflow, and this report.

## 4. Files modified

- Rust dependency features/versions and lockfile; migration registry/database; model domain exports; credential boundary; worker model enforcement; application state; command registration; and module exports.
- pnpm Vite/Vitest patch versions and lockfile; root quality scripts; UI reducer performance test and Milestone 8 label.
- Tauri bundle resources/NSIS/current-user/zlib/WebView2-skip configuration and CI quality gates.
- README, security/license policy, resource manifest guidance, dependency inventory, architecture, pipeline, threat model, test strategy, release docs, and roadmap status.
- No production runtime package was added. Existing Serde/time/Vite/Vitest lines were minimally upgraded only after license and advisory review.

## 5. Commands executed

- Baseline/final: `pnpm lint`, `pnpm typecheck`, `pnpm build`, and `pnpm test`.
- Rust: focused Milestone 8 tests, `cargo fmt --check`, and `cargo clippy --workspace --all-targets --locked -- -D warnings`.
- Release: `pnpm schemas:check`, `pnpm release:audit`, `pnpm audit --audit-level high`, and RustSec `cargo audit` with two documented exact-ID exceptions.
- Packaging: Tauri release build without bundle, actual NSIS bundle, binary scan for the excluded WebView2 download URL, silent current-user install, installed resource check, hidden application startup, process stop, and silent uninstall.
- Supply chain: npm metadata/license review, Rust crate metadata review, SBOM/notices regeneration/check, and official upstream license/model/installer research.

The shell prepended the existing `.venv/Scripts` and installed Rust directories because the session PATH exposes Windows Store Python and omits Cargo. NSIS and RustSec needed approved network access for their official tools/databases.

## 6. Build results

- Web TypeScript/Vite 7.3.5 production build: passed.
- Rust workspace locked debug build and optimized Tauri release build: passed.
- Local NSIS current-user/zlib/WebView2-skip installer: built successfully, 5,465,535 bytes. Installed application executable: 15,372,800 bytes.
- Silent install found the exact `vietdub-studio.exe` and both model manifests; the installed application remained running through the startup smoke; silent uninstall removed the executable.
- TypeScript strict typecheck and Python strict mypy across 45 source files: passed. ESLint, Ruff, rustfmt, and Clippy warnings-as-errors: passed.
- All 17 JSON Schemas passed Draft 2020-12 validation. Release audit reproduced 576 components: 286 Windows-target Cargo, 285 locked npm, and 5 pinned Python packages.
- Authenticode build is intentionally not reported as passed; the local artifact is unsigned and the protected workflow was not run.

## 7. Test results

- Web: 12 passed.
- Rust: 106 passed (25 unit, 8 Milestone 8, 7 Milestone 5, 6 Milestone 4, 11 Milestone 1, 7 Milestone 7, 6 Milestone 6, 20 Milestone 3, 11 Milestone 2, 5 worker protocol).
- Python: 67 passed.
- Python integration/security: 5 passed.
- Total reported test cases: **190 passed, 0 failed**.

Milestone 8 coverage includes model catalog/installation corruption and traversal refusal, unresolved-license refusal, Windows credential round-trip when a vault exists or fail-closed behavior when unavailable, log redaction/rotation/disable, simulated crash marker and contained partial cleanup, unsigned/checksum/update-policy refusal, SBOM uniqueness/license/direct-component reconciliation, 1,000-job recovery, 64 MiB hashing, UI reducer budget, schema/catalog/installer/workflow checks, and all earlier cancellation/path/process-tree isolation suites.

## 8. Security checks

- Initial npm audit found one critical and three high findings. Vite 7.3.5 and Vitest 3.2.6, both MIT, removed all high/critical findings. One low development-server-only esbuild advisory remains documented and expiring.
- RustSec `time` was upgraded to 0.3.47. Two high `quick-xml` findings are reachable through `plist`/Tauri build tooling only, not Windows user-input parsing; no compatible `plist` line exists, so exact-ID exceptions expire 2026-09-02 and remain release blockers after that date unless renewed with evidence.
- SBOM/notices are based on Cargo metadata filtered to `x86_64-pc-windows-msvc`, locked pnpm packages, and pinned Python metadata, so output does not vary with stale package caches. Missing, unknown, GPL/AGPL/SSPL/BUSL/Elastic, duplicate, or unreconciled direct components fail the audit.
- Installer configuration has no updater and sets WebView2 mode to `skip`; the generated installer contains no WebView2 bootstrapper URL. Models/tools are never silently downloaded. Unsigned, wrong-name, wrong-size, wrong-checksum, symlink, automatic-update, or untrusted release artifacts fail closed.
- Credential values have redacted `Debug`, bounded reference/value rules, drop-time byte overwrite, native Credential Manager persistence, and no plaintext fallback. Logs accept restricted metadata and redact secret/path patterns.
- The repository still has no initial Git commit, so historical diff/provenance cannot be independently reconstructed from Git.

## 9. Known limitations

- A trusted Authenticode PFX/password was not provided. The protected build-sign job, trusted timestamp, signed manifest, `signtool /pa` evidence, and separate fresh-runner install/uninstall remain unexecuted. This is the sole acceptance blocker that requires external authority/material.
- The headless agent's Windows logon session did not expose a usable Credential Manager vault. The adapter failed closed as designed; a real interactive Windows release acceptance must additionally inspect a successful credential round trip and confirm no plaintext in SQLite/logs/project data.
- The current advisory exceptions and low npm finding are bounded in `docs/release/security-exceptions.md`; they require review by 2026-09-02.
- Supported machines must already have WebView2 Runtime. The installer deliberately refuses to download it. FFmpeg/ffprobe/yt-dlp and model weights remain separate explicit verified installations.
- The unsigned local NSIS artifact under `target/` is build output only and must not be published. No release manifest was generated for it.

## 10. Next milestone

There is no Milestone 9. Do not expand product scope. Milestones 0-7 remain verified; Milestone 8 implementation is complete but acceptance remains pending. By strict accepted-milestone count the project remains 7/8 (87.5%) until the protected release workflow passes. The only next action is to supply authorized signing secrets through GitHub, run `.github/workflows/release.yml`, and append immutable signed-artifact and clean-runner evidence here.

## 11. Exact continuation prompt

```text
Tiếp tục chỉ phần nghiệm thu phát hành của Milestone 8; không thêm tính năng hoặc Milestone 9.

Kiểm tra lại AGENTS.md, docs/roadmap/implementation-plan.md và docs/roadmap/milestone-8-verification.md.
Cấu hình GitHub protected secrets WINDOWS_CERTIFICATE_BASE64 và WINDOWS_CERTIFICATE_PASSWORD bằng certificate Authenticode hợp lệ, sau đó chạy thủ công .github/workflows/release.yml.
Không đưa certificate/private key vào repository, log hoặc artifact.
Chỉ khi build-sign và clean-machine-acceptance đều thành công, ghi lại tên/size/SHA-256 release manifest, kết quả signtool cho installer và installed executable, kết quả install/start/uninstall trên runner sạch, rồi đánh dấu Milestone 8 fully accepted.
Nếu gate thất bại, chỉ sửa lỗi thuộc Milestone 8 và chạy lại toàn bộ lint/typecheck/build/test/audit; không mở rộng phạm vi.
```
