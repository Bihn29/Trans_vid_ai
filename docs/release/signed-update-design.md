# Signed release and update design

## Policy

VietDub Studio does not poll, download, or install updates in the background. The application contains no updater plugin and `ReleaseManifest.automaticUpdates` must be `false`. NSIS sets WebView2 installation mode to `skip`, so installation also performs no implicit bootstrapper download; supported Windows machines must already provide WebView2 Runtime. A release is promoted only by the manual Windows release workflow after all verification gates pass.

## Artifact flow

1. A pinned clean Windows runner installs dependencies from committed lockfiles and runs lint, typecheck, build, tests, schema validation, and `release:audit`.
2. Tauri creates the current-user NSIS installer using zlib compression and without a WebView2 downloader. FFmpeg, ffprobe, yt-dlp, model weights, credentials, and private media are not bundled.
3. The workflow refuses to continue unless an Authenticode PFX and password are supplied as protected release secrets. Tauri signs the patched application and the finished installer with SHA-256 and a trusted RFC 3161 timestamp, then both are checked with Windows `signtool verify /pa`. The imported certificate and temporary PFX are removed in a `finally` block.
4. A release manifest records exact filename, byte size, and lowercase SHA-256. The installer and manifest are uploaded only after signing and verification.
5. The clean-machine job silently installs the signed artifact in its isolated runner account, verifies the installed executable's Authenticode trust and expected files, then uninstalls it. No unsigned artifact is published.

## Application verification

`verify_release_artifact` requires an absolute regular non-symlink `.exe` or `.msi`, exact filename/size/SHA-256, `authenticodeRequired=true`, and `automaticUpdates=false`. Windows verifies Authenticode through `WinVerifyTrust` with UI disabled and cache-only revocation retrieval. Any mismatch, unsigned file, unsupported platform, or unavailable trust chain fails closed.

The application deliberately does not execute a verified installer. The user remains responsible for selecting and launching an update after reviewing the release notes. Adding automatic update transport would require a new ADR, threat-model review, pinned dependency/license audit, rollback design, and explicit authorization.
