# Threat model

## Assets

- Private source video, extracted audio, transcripts, translations, and rendered output
- Provider credentials and OS credential-store handles
- Integrity of bundled executables, models, project state, and artifacts
- Availability of the desktop process and other queued projects

## Adversaries and untrusted inputs

Remote pages, redirects, filenames, media containers, subtitles, logos, worker output, model archives, provider responses, and user-pasted paths/URLs are untrusted. A malicious project may attempt path traversal, command injection, SSRF, excessive resource use, secret exfiltration, or parser exploitation.

## Boundaries and controls

| Boundary | Primary threats | Required controls |
| --- | --- | --- |
| UI to Rust IPC | oversized/invalid fields, raw tool options | typed commands, length/range enums, no raw FFmpeg/yt-dlp arguments |
| URL to downloader | SSRF, redirect rebinding, huge download | HTTPS and domain allowlists, DNS/IP validation at every redirect, byte/time/redirect caps |
| Rust to process | command injection, orphan process, log leakage | no shell, fixed executable, argument arrays, tracked PID, timeout/cancel, bounded redacted output |
| Rust to worker | malformed JSON, spoofed request, hangs | UUID correlation, versioned schemas, max line size, terminal-state rules, kill and wait |
| Project filesystem | traversal, symlink escape, overwrite | generated UUID names, canonical containment checks, relative DB paths, immutable source |
| Cloud provider | privacy loss, secret exposure | explicit provider choice, least data, credential store, cloud indicator, local-only mode |
| Binary/model supply chain | tampering, license breach | approved HTTPS source, manifest/version/license/SHA-256, consent, verify before execution |

## Milestone 0 security coverage

Implemented foundations are schema validation, bounded JSON Lines, timeout/cancellation, checked child exit, safe worker failures, pinned direct dependencies, and migration transactions. URL/network, project path, media parser sandboxing, secret-store integration, signature/update handling, and process-tree termination require later milestones and must not be represented as complete.

## Milestone 1 security coverage

Project directories use generated UUIDs beneath one canonical root. Stored artifact paths are ASCII relative paths; absolute paths, `..`, alternate separators, drive syntax, trailing dot/space aliases, Windows device names, and canonical symlink escapes are rejected. Registry reuse requires matching SHA-256 and byte size, not existence. Project deletion is recoverable through a contained trash move before the database row is removed.

Job claims are transactional, concurrency-bounded, and isolated by per-job cancellation tokens. Pause/cancel intent survives restart. Recovery never converts interrupted work into success. Cache keys use fixed domain-separated, length-prefixed serialization, and dependency invalidation refuses to race a running affected stage.

Milestone 1 adds no network client, downloader, media parser, cloud adapter, TTS/ASR engine, or production fake. SSRF, remote download limits, complex media sandboxing, credentials, and descendant process trees remain later controls.

## Milestone 2 security coverage

Local import rejects relative inputs, symlinks, unsupported extensions, empty files, second-source replacement, and configured-size overflow. Copying uses a create-new temporary file, streaming cap, flush, generated destination, atomic promotion, read-only source, registry hash/size verification, and cleanup on failure. Filenames containing shell metacharacters remain ordinary filesystem or argv values.

Remote contracts accept only HTTPS URLs for exact Douyin, Bilibili, YouTube, or TikTok host allowlists. Credentials, fragments, non-default ports, IP literals, cross-site redirects, excessive redirects, empty DNS answers, and loopback/private/link-local/reserved/documentation/multicast addresses are rejected. A downloader receives a resolved endpoint and must connect to one of its validated socket addresses; every redirect repeats URL, DNS, and address validation. No production downloader or network call is bundled in Milestone 2.

External tools require a canonical non-symlink executable and expected SHA-256, rechecked immediately before spawn. The supervisor never uses a shell, bounds both output streams, redacts sensitive stderr, enforces timeout/cancellation, checks exit status and required output, kills and waits, and uses a Windows Job Object to terminate descendants. FFmpeg/ffprobe binaries remain external and require a separate build/license manifest.

## Abuse cases to test

- Shell metacharacters in filenames remain one inert argument.
- `..`, absolute paths, alternate separators, symlinks, and reserved device names cannot escape a project.
- Loopback/private/link-local URLs and redirects are rejected before connection.
- Oversized, malformed, duplicate-terminal, wrong-request-ID, silent, and hanging workers fail safely.
- Cancellation for project A does not stop project B.
- Secrets resembling authorization headers or API keys are redacted from logs and UI errors.

## Milestone 3 security coverage

ASR receives only a verified `original_audio` artifact owned by the same project. Rust sets the canonical project root as the worker working directory, and both Rust and Python reject absolute paths, traversal, alternate separators, symlinks, and paths resolving outside that root. A worker-declared transcript is not accepted until the file exists and its reported SHA-256 and size match independent Rust verification and Artifact Registry registration.

Model consent is checked on the same `WorkerManager.client_for_stage` path that creates the process. Both the preferred and fallback model require consent. Local installations require a bounded `vietdub-model.json` with an approved HTTPS source and file-level size/SHA-256 records; provider and license must match the consent snapshot. Model adapters accept existing local directories only, and faster-whisper uses local-files-only loading. The deterministic ASR implementation exists only under test fixtures.

Transcript IPC operations bind segment IDs to the supplied project ID. Timestamp overlap, unsafe split/merge, cross-project merge, and corrupt regional replacement fail without partially replacing the stored transcript. Downstream invalidation is persisted before an editor mutation so stale completed outputs cannot be reused.

## Residual risks

AI model execution and media codecs process complex untrusted data and may contain native vulnerabilities. Process lifecycle controls and verified local manifests do not provide an OS sandbox for Python/native model code. Release design should isolate writable locations, minimize bundled features, update only with explicit verified packages, and document offline operation.

SQLite and the filesystem cannot share one atomic transaction. Project JSON is a derived snapshot recoverable from SQLite, and delete uses a compensating trash restore on database failure. Artifact producers in later milestones must write a temporary file, flush it, promote it inside the project, and only then register it.

## Milestone 5 security coverage

The production TTS worker accepts only an approved provider ID, built-in catalog voice ID, reviewed translated text, bounded speed/retry values, and explicit cloud consent. There is no custom sample, embedding, consent-recording, or voice-cloning input. The OpenAI-compatible adapter requires HTTPS, rejects credentials, fragments, non-default ports, non-global literal IPs and redirects, caps the response at 32 MiB, and returns only PCM WAV.

Credentials are retrieved through the existing Rust credential-store boundary immediately before execution and are not represented in the TTS schema, database, artifact metadata, metrics, or safe errors. The production application still fails closed because OS credential storage is deferred to Milestone 8.

Python and Rust independently validate WAV structure and metadata. Rust additionally enforces project-relative containment, rejects symlinks, recomputes SHA-256 and byte size, and registers the artifact before marking a segment complete. Cache hits repeat Artifact Registry verification; corrupt or missing audio is reset to pending and regenerated. Per-job cancellation and per-segment persistence prevent a failing or cancelled request from corrupting another project's state or discarding already verified segment output.

The cloud service remains a privacy boundary: translated text is sent off-device after explicit consent. Provider retention, account policy, and service availability are residual external risks. Audio time-stretching, separation, and mixing are not present in Milestone 5 and therefore are not claimed as covered.

## Milestone 6 security coverage

The approved separation engine is bundled clean-room Python source with no model, package install, network call, credential, or silent download. Its manifest explicitly records local processing, install/consent state, distribution mode, version, and license status. Demucs was not approved because the upstream pretrained-weight license remains unclear even though the source repository declares its code MIT.

The separation worker accepts one bounded project-relative PCM WAV and rejects traversal, symlinks, malformed/oversized audio, unsupported formats, unsafe thresholds, and output replacement. Rust independently checks descriptor count/types, containment, SHA-256, byte size, PCM metadata, and exact stem alignment. An invalid or unavailable provider falls back to explicitly labeled attenuation of the already verified source; a cancellation does not.

The Rust mixer accepts only Artifact Registry entries owned by the same project. It bounds gains, fades, normalization, limiter peak, WAV size, sample rate, and timeline arithmetic. It checks cancellation throughout assembly, writes through a create-new temporary file plus flush/rename, and registers only a duration-aligned, post-limit unclipped result. TTS files are read-only inputs and cancellation/failure cannot mutate or unregister them.

The DSP pipeline is not an OS sandbox. Large but valid PCM files can still consume significant memory, and the energy-mask engine does not provide neural separation quality. The current 512 MiB file limit, worker timeout, per-job token, and fixed mono PCM contract bound but do not eliminate denial-of-service risk.

## Milestone 7 security coverage

Composer IPC accepts only a bounded typed config. Raw tool arguments and filter fragments are absent. Every referenced artifact is verified for project ownership, kind, relative containment, SHA-256, and byte size. Text and subtitle payloads are stored in generated contained files so punctuation cannot become filter syntax; filter paths use the restricted ASCII relative-path grammar.

The render process uses the approved-executable checksum boundary, argument arrays, canonical project working directory, timeout, bounded output, and per-job cancellation. Partial files are request-unique and cleaned on failure/cancellation. A result is registered only after ffprobe confirms non-empty video/audio streams, target dimensions, and duration tolerance. Source and mixed audio remain immutable inputs.

External SRT/WAV export is an explicit user action. It permits only the matching verified artifact kinds/extensions, rejects relative destinations and symlink parents, uses create-new semantics, and never overwrites. FFmpeg/ffprobe still process complex untrusted media without an OS codec sandbox; production remains fail-closed until reviewed binary manifests are supplied.

## Milestone 8 security coverage

The model manager loads only bounded, non-symlink catalog JSON from bundled resources. An install must be a canonical non-symlink directory with an exact catalog identity and 1â€“4096 restricted relative files; every non-empty file is verified by registered size and lowercase SHA-256. Catalog entries with unresolved local-use approval fail before persistence or worker launch. Neither the catalog nor installer contains model weights.

Provider credential IPC restricts service names and credential reference characters. Secret values are wrapped with redacted diagnostics, zeroed on drop, and written only through Windows Credential Manager. If a credential vault is unavailable, the operation fails; there is no SQLite, file, log, or environment fallback. IPC exposes only availability, never the stored value.

Diagnostics accept metadata only, cap fields and values, redact secret/path patterns, and rotate within persisted file-count and byte limits. Disabling logging is effective before a record is opened. Crash cleanup canonicalizes the project root, skips symlinks/directories beyond the bounded walk, and deletes only generated dot-prefixed `.partial` files, so unrelated user files and other projects remain isolated.

The release workflow has read-only repository permissions and requires protected certificate secrets. Tauri signs the patched application during bundling and the installer after packaging, the job removes the imported certificate and temporary PFX, both signatures are verified, and the signed installer is bound to a SHA-256/size manifest. Installation/uninstallation runs on a separate fresh Windows runner. The application has no updater; checksum or Authenticode failure is a hard refusal. Remaining advisory exceptions are explicit and expiring in `docs/release/security-exceptions.md`.
