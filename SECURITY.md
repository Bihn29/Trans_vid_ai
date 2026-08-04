# Security policy

## Reporting

Do not open a public issue for a suspected vulnerability. Send a private report to the repository owner with affected versions, impact, reproduction steps, and suggested mitigations. Do not include real credentials or private media. A dedicated disclosure address will be added before public distribution.

## Security invariants

- Rust is the only layer allowed to launch FFmpeg, ffprobe, yt-dlp, or Python workers, using an executable plus an argument array. Shell execution and user-supplied raw options are forbidden.
- Remote import is HTTPS-only, allowlisted by adapter, revalidated after redirects, and rejects loopback, link-local, private, and otherwise non-public destinations.
- Every project-relative path is validated and resolved beneath its project root. Remote filenames are never trusted.
- Worker messages are versioned, schema-validated, size-limited JSON Lines. Timeouts, cancellation, exit status, and bounded stderr are mandatory.
- Credentials belong in the OS credential manager. Logs, SQLite, project files, crash reports, and UI errors must be secret-free.
- Models and external tools require an approved source, version, license metadata, and SHA-256 before use. The application and installer perform no model/tool/WebView2 download; installation is an explicit user action.
- Telemetry defaults off. Privacy mode must omit transcript content from logs.

See `docs/threat-model/threat-model.md` for implemented trust boundaries, residual risks, and release controls. Temporary advisory exceptions are exact-ID, scoped, and expiring in `docs/release/security-exceptions.md`.
