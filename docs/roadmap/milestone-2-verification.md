# Milestone 2 verification

## 1. Scope

- Date: 2026-08-01
- Platform: Windows, Node 22.23.2, pnpm 9.15.4, Python 3.11.9, Rust/Cargo 1.97.1
- Implemented only immutable import, guarded downloader contracts, probing, proxy/audio preparation, and bounded tool supervision.
- No Milestone 3 ASR/transcript/model work is present.

## 2. Pre-change baseline

The Milestone 1 workspace passed production build, lint, strict type checks, 3 Web tests, 26 Rust tests, 11 Python tests, and the real echo-worker integration test before Milestone 2 code changed.

## 3. Buildable increments

1. Add reviewed URL/Windows APIs and the generic process supervisor.
2. Add immutable local import and typed Tauri command.
3. Add HTTPS/site/DNS/IP/redirect policy and downloader contracts.
4. Add ffprobe/FFmpeg adapters plus registered metadata/proxy/audio outputs.
5. Add generated fixtures, security/integration tests, schema, ADR, and acceptance evidence.

Each increment compiled before the next began.

## 4. Local import

The local path never calls a downloader or child process. It validates an absolute regular non-symlink supported file, streams through a byte cap into a create-new partial, flushes, promotes to a UUID filename, marks the copy read-only, registers SHA-256/size, and attaches the source to the project. A second source is rejected.

## 5. Remote contracts

Separate contracts cover Douyin, Bilibili, YouTube, and TikTok. Only HTTPS exact-host URLs without credentials, fragments, or custom ports pass. DNS results must all be public; redirects are bounded, remain within one site family, and repeat validation. Providers receive validated socket addresses. Remote filenames are never project paths. No concrete downloader/network call is shipped.

## 6. Tool supervision

External executables require canonical paths and reviewed SHA-256 values, rechecked immediately before spawn. The supervisor uses argv arrays, bounded streams, stderr redaction, timeout/cancellation, checked status, kill-and-wait, and Windows Job Object descendant termination.

## 7. Media preparation

ffprobe emits bounded JSON validated into `MediaMetadata` and `media-metadata.schema.json`. Fixed FFmpeg adapters create a 1280-wide H.264/AAC proxy and mono 16 kHz PCM WAV. Success additionally requires a non-empty contained output and artifact registration.

## 8. Tests and fixtures

Offline tests cover metacharacter filenames, traversal, size/type/source limits, local/downloader isolation, four site contracts, SSRF, redirects, executable mutation, output caps, redaction, timeout, cancellation, descendant termination, probe parsing, proxy/audio artifacts, non-zero exit, and missing output. Fixtures are generated/CC0 and contain no third-party media.

## 9. Dependency and license review

Direct declarations add `url 2.5.8` and Windows-only `windows-sys 0.61.2`, both already present in the lockfile. Local manifests declare MIT/Apache-2.0. The URL version is beyond the IDNA advisory's patched recommendation. FFmpeg, ffprobe, and yt-dlp are not bundled; an exact build manifest remains mandatory before distribution.

## 10. Quality gates

- `pnpm lint`: passed ESLint, rustfmt check, and Ruff.
- `pnpm typecheck`: passed strict TypeScript and mypy for eight Python source files.
- `pnpm build`: passed Vite production build and locked Rust workspace build.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: passed.
- Web: 3 passed.
- Rust: 44 passed: 17 unit, 11 Milestone 1, 11 Milestone 2 integration/security, and 5 worker-protocol tests.
- Python: 11 passed.
- Real echo-worker subprocess integration: 1 passed.
- Media metadata schema, generated fixture, dependency, no-shell, and Milestone 3 scope scans passed.

## 11. Residual risk and stop condition

Native codec parsing is not OS-sandboxed. Remote contracts deliberately ship without a concrete downloader until its transport can prove connection to validated addresses and its terms/license are approved. SQLite and filesystem promotion still use compensating cleanup rather than one cross-resource transaction.

Milestone 2 stops here. Milestone 3 requires explicit authorization.
