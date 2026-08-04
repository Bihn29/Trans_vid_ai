# Windows release checklist

## Source and dependencies

- [ ] Release commit/tag exists and the worktree is clean.
- [ ] Node, pnpm, Python, Rust, and Tauri versions match the committed policy.
- [ ] Frozen/locked installs complete without changing lockfiles.
- [ ] Dependency advisories and licenses are reviewed; `pnpm release:audit` passes.
- [ ] `sbom.cdx.json` and `THIRD_PARTY_NOTICES.md` exactly reconcile with the release environment.
- [ ] All model/tool/font/media manifests have reviewed source, version, license, size, and checksum.
- [ ] No FFmpeg, ffprobe, yt-dlp, cloud SDK, credential, or model weight was added implicitly.

## Quality and privacy

- [ ] Lint, strict typecheck, locked build, Clippy warnings-as-errors, all tests, and schemas pass.
- [ ] Deterministic E2E/security suite passes without network or paid APIs.
- [ ] Startup/recovery/hash/UI performance budgets pass on the reference runner.
- [ ] Credential inspection finds provider secrets only in Windows Credential Manager.
- [ ] SQLite, project files, generated logs, diagnostics, SBOM, and installer contain no fixture secret.
- [ ] Log rotation, redaction, crash marker, stale partial cleanup, and queue recovery are verified.

## Signing and clean-machine acceptance

- [ ] NSIS installer uses current-user mode and zlib compression.
- [ ] Installer skips WebView2 download; the supported Windows image must provide WebView2 Runtime.
- [ ] Protected Authenticode certificate and password are present only in the release job.
- [ ] Tauri signs the patched application and installer with SHA-256 and a trusted RFC 3161 timestamp.
- [ ] `signtool verify /pa` succeeds for installer and installed executable.
- [ ] Release manifest exact filename, SHA-256, size, channel, and version match the signed installer.
- [ ] Fresh Windows runner installs, starts/verifies files, and uninstalls without admin rights.
- [ ] No unsigned artifact or private certificate material is uploaded.

## Publication and rollback

- [ ] Human approver reviews release notes, SBOM, notices, manifest, and clean-machine evidence.
- [ ] Signed installer, release manifest, SBOM, notices, and checksums are published together.
- [ ] Previous signed installer remains available for manual rollback.
- [ ] Application performs no automatic download/update; users explicitly choose installation.
