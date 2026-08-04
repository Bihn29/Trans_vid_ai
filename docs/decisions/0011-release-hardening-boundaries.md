# 0011. Release hardening boundaries

- **Status:** Approved
- **Deciders:** Antigravity Team
- **Date:** 2026-08-02

## Context

Milestone 8 must make model installation, credentials, diagnostics, restart recovery, and Windows release artifacts verifiable without adding an implicit network or trust path. These concerns cross the existing filesystem, persistence, process, and distribution boundaries and therefore require an explicit architecture decision.

## Decision

1. The bundled model catalog contains metadata only. Models are user-provided, require explicit consent and installation, and are verified from a contained `vietdub-model.json` plus per-file SHA-256 and size before worker creation. No model is distributed by the installer.
2. Provider secrets are stored only as generic credentials in Windows Credential Manager. SQLite stores no secret or fallback copy. A missing logon-session vault fails closed.
3. Diagnostics are opt-out, metadata-only JSON Lines. Event codes and keys are restricted, values are bounded and redacted, and persisted settings cap both file size and retained file count.
4. SQLite runtime-session markers detect an unclean prior exit. Startup marks interrupted sessions recovered, invokes existing queue recovery, and removes only contained, non-symlink, generated partial files. The existing Windows Job Object remains responsible for kill-on-parent-close process trees.
5. Distribution is a manual NSIS current-user install with zlib compression and WebView2 installation mode `skip`. There is no updater, bootstrapper, or background download; supported Windows machines must already provide WebView2 Runtime. A protected certificate is imported only on the ephemeral release runner; Tauri signs the patched application binary during bundling and then signs the installer; a separate fresh Windows runner verifies and installs both.
6. A release manifest binds the exact signed filename, size, and SHA-256 and requires Authenticode while forbidding automatic updates. Verification fails closed on checksum, size, filename, symlink, or signature mismatch.
7. The release environment generates a deterministic CycloneDX SBOM and third-party notices from the locked Rust, pnpm, and Python dependency sets. Current advisories are gated separately and any exception is named, scoped, dated, and expiring.

## Consequences

- No model weight, media tool, cloud SDK, certificate, secret, or automatic updater is introduced.
- Windows Credential Manager and Authenticode verification are Windows-specific infrastructure behind existing Rust boundaries; provider and pipeline domain contracts remain unchanged.
- A signed release cannot be produced locally without protected certificate material. Only the manual protected release workflow is authoritative evidence for signed-artifact and clean-machine acceptance.
- Two temporary `quick-xml` build-tool advisory exceptions are recorded in `docs/release/security-exceptions.md`; they do not apply to user media/model parsing and must be removed when the upstream `plist` dependency accepts the patched line.
