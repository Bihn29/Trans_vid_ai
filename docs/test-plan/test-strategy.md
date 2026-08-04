# Test strategy

## Principles

Tests are deterministic, offline by default, and never download large models or call paid APIs. Test providers implement the same contracts as production providers but are compiled or configured only for tests. A failed subprocess cannot be converted to success by a plausible output file.

## Layers

- **Rust unit:** stage state/invalidation, cache identity, project path containment, artifact hashing, queue transitions, migrations, argument construction, bounded worker protocol, resume, and cancellation isolation.
- **TypeScript:** components, stores/reducers, form validation, subtitle edits, undo/redo, and pipeline-state rendering.
- **Python:** protocol/schema validation, normalized ASR segments, translation parsing, TTS cache keys, and provider contract suites.
- **Integration:** short generated fixture through import/probe/audio/fake worker/subtitle/mix/render; Milestone 0 begins with a real Rust-to-Python echo exchange.
- **Security:** metacharacter filenames, path traversal, SSRF/redirect cases, oversized/malformed/hanging workers, timeouts, and redaction.
- **E2E:** create/import, deterministic pipeline, review checkpoint, edit, resume, render, and verify output.

## Milestone 0 matrix

| Capability | Verification |
| --- | --- |
| React strict scaffold | TypeScript build and component/store tests |
| SQLite migration base | in-memory Rust migration tests and idempotent re-run |
| Request/response schemas | Python positive/negative tests and Rust validation |
| Worker happy path | Rust launches Python echo worker and observes progress/completed |
| Safe failure | worker emits schema-valid failed event; Rust returns a typed failure |
| Timeout/cancel | sleeping worker is killed and reaped within bounded time |
| Output limit | oversized JSON line is rejected |
| No shell | command construction test verifies executable and discrete arguments |

## Milestone 1 matrix

| Capability | Verification |
| --- | --- |
| Migration upgrade | version-1 database upgrades transactionally to version 2; future versions are rejected |
| Project CRUD/isolation | UUID layout and derived snapshot are created; update/list/delete work; traversal and Windows aliases fail |
| Symlink containment | canonical path resolution rejects an escaping directory symlink when the test account can create one |
| Artifact integrity | registration stores relative path, SHA-256, and size; mutation and deletion are detected |
| Cache identity | recursive metadata order is stable, a golden digest prevents drift, scope mismatch misses, corrupt output invalidates |
| Targeted invalidation | changed segment invalidates its dependent runs and project tail without touching another segment or ASR |
| Invalidation race | an affected running stage blocks the transaction without partial invalidation |
| Persistent priority queue | higher priority claims first; runtime concurrency changes are enforced |
| Pause/resume/retry | valid transitions succeed; retry creates linked Job and StageRun attempts and preserves history |
| Cancellation isolation | cancelling one running job signals only its token and leaves the other job running |
| Restart recovery | pause/cancel/requeue decisions survive queue recreation and actual SQLite close/reopen |
| Review checkpoint | provider review outcome releases the queue slot and persists `review_required` |
| Test-only provider | deterministic provider exists only in `tests/milestone_one.rs`; production has only the contract |

## Milestone 2 matrix

| Capability | Verification |
| --- | --- |
| Immutable local import | generated project filename, read-only promoted copy, hash/size registry, source attachment |
| Local safety | metacharacter filename remains inert; symlink/path/type/empty/size/second-source failures are typed |
| Failure cleanup | overflow and failed promotion leave no partial source; failed derived output is not registered |
| Offline independence | local import succeeds while a downloader contract is configured to fail and is never called |
| Site contracts | separate Douyin, Bilibili, YouTube, and TikTok contracts reject another site's URL |
| SSRF/redirect policy | HTTPS, exact hosts, credentials, ports, cross-site redirects, redirect count, and public IPs |
| Executable integrity | relative/missing tools and post-approval SHA-256 mutation are rejected before spawn |
| Process supervision | discrete argv, output caps, redaction, checked exit, timeout, cancellation, kill-and-wait |
| Windows descendants | timeout Job Object test proves a spawned descendant cannot write its delayed sentinel |
| ffprobe | bounded JSON parses to versioned metadata; missing/extreme fields fail |
| FFmpeg | fixed proxy/audio arguments produce contained registered artifacts in offline fake-tool integration |
| Checked success | non-zero exit and zero exit without required output both fail |
| Fixtures | generated temporary media and CC0 test tool; no third-party media or network required |

## Milestone 3 matrix

| Capability | Verification |
| --- | --- |
| Model consent | consent record creation/lookup, consent enforcement before worker launch |
| ASR provider contract | normalized `AsrSegment` structure, `end_ms > start_ms` invariant, confidence validation |
| Test-only ASR fixture | Rust-to-Python pipeline fixture lives under `tests/fixtures/`; production workers contain no deterministic provider |
| Installed model integrity | both primary/fallback consent required; local manifest path, size, and SHA-256 verified; corruption rejected |
| Segmentation normalization | merging ultra-short segments (<200ms) and splitting ultra-long segments (>15s) |
| QC warnings | detection of overlap, empty text, long duration, repetition, silence gap, low confidence |
| Artifact handshake | worker writes a real transcript; Rust verifies descriptor/file hash and size before registry insertion |
| Segment persistence | SQLite migration 0003, transactional replace, rollback, regional replace, resequencing, sequence uniqueness |
| Transcript editor ops | project isolation, timestamp/speaker edit, split edge cases, adjacent merge, source hash computation |
| Transcript review checkpoint | persisted `review_required` job consumes no slot; approval completes the checkpoint |
| Targeted invalidation | real StageRun rows are invalidated after source edits without touching ASR; regional rerun preserves unaffected segments |

## Gates

Every milestone must pass formatting/lint, strict type checks, build, unit tests, and its integration suite. Security-critical boundaries require negative tests. CI uses pinned lockfiles and a clean Windows runner. Known environment blockers are reported as blockers, never marked as passes.

## Milestone 5 matrix

| Capability | Verification |
| --- | --- |
| Migration and persistence | schema version 5 creates assignment and per-segment run tables; migration remains transactional/idempotent |
| Provider-neutral catalog | approved voices expose disclosure metadata; the contract suite uses no paid call or model download |
| Assignment routing | two speakers resolve to distinct voices; segment overrides speaker; project ownership is enforced |
| Cache identity and integrity | provider/voice/model/speed/text identity is deterministic; distinct voices differ; corrupted registered WAV is regenerated |
| Audio handshake | Python and Rust verify PCM WAV, SHA-256, size, sample rate, channel count, bit depth, duration, and contained relative path |
| Preview isolation | preview registers independently and does not replace selected segment audio |
| Failure isolation and retry | one failed segment preserves completed peers; retry reuses verified peers and completes only pending work |
| Restart recovery | persisted `running` TTS segment records return to `pending`; completed records remain reusable |
| Duration fitting | safe rate is clamped to 0.85–1.20; long audio requests translation shortening; excessive slowdown warns |
| Cloud and credentials | explicit consent, HTTPS policy, disabled redirects, response cap, redacted secret boundary, production fail-closed default |
| No voice cloning | schemas, catalog, worker, commands, and production source expose no custom voice or sample input |
| Test-only provider | deterministic WAV worker exists only in `tests/fixtures/tts_workers` |

## Milestone 6 matrix

| Capability | Verification |
| --- | --- |
| Migration/settings | schema version 6 persists bounded project-scoped background/voice/music/original/duck/fade/normalization/limiter controls |
| Approved engine metadata | manifest declares built-in source, no install/download/model/consent/cloud transfer; no new dependency is present |
| Separation contract | deterministic energy mask returns reconstructable, equal-length PCM stems and rejects malformed input/config |
| Artifact handshake | worker stem paths, hashes, sizes, PCM metadata, ownership, and exact alignment are independently verified by Rust |
| Fallback | worker failure produces explicitly labeled 25% attenuation artifacts; cancellation does not become success |
| Timeline alignment | segment TTS is fitted and placed by sample index; mixed output duration equals the background within one millisecond |
| Deterministic plan | identical artifact hashes, timestamps, and settings produce the same timeline hash and output WAV hash |
| Mix controls | background, dubbed voice, music, original voice, ducking, and fades are typed/range-checked |
| Loudness/limiter/QC | bounded RMS normalization and peak limiting produce zero post-limit clipped samples and persisted audio metadata |
| TTS preservation | successful TTS artifacts verify with unchanged hashes after mix success, provider fallback, and cancellation |
| Project/path isolation | cross-project artifact kinds/ownership and traversal are rejected; writes stay in generated project audio directories |
| Scope guard | no composer worker, subtitle renderer, video render, or raw FFmpeg filter input exists |

## Milestone 7 matrix

| Capability | Verification |
| --- | --- |
| Migration/config | schema version 7 persists project-scoped bounded typed composer config |
| Filter safety | punctuation-rich user text never appears in filter syntax; every referenced path passes the restricted relative-path parser |
| Bounds/aspects | crop and overlay bounds reject overflow; source, 16:9, 1:1, and 9:16 produce fixed even dimensions |
| Subtitles | deterministic trim/speed-adjusted SRT; soft mode maps `mov_text`; burned mode references the contained SRT |
| Composition | typed trim/crop/scale/pad/blur/flip/speed, cover, text, logo/watermark, and draft/final settings; no raw option field |
| Identity/invalidation | source/audio/overlay/subtitle/config dependencies produce a SHA-256 identity; composition invalidates the render tail |
| Render/QC | fake approved tool produces a registered MP4; independent probe verifies video/audio, dimensions, and duration |
| Immutability/integrity | verified source hash and bytes remain unchanged; corrupt export input is rejected |
| Cancellation/retry | cancellation removes partial render and affects only its token; retry creates a new attempt and completes |
| Export | verified SRT/WAV only, fixed extension, canonical parent, create-new, no overwrite |
| UI | typed aspect/subtitle/preset/speed/blur controls expose no FFmpeg input |

## Milestone 8 matrix

| Capability | Verification |
| --- | --- |
| Migration/recovery | schema version 8 creates runtime session, verified model, and privacy settings state; simulated unclean restart is detected and later marked clean |
| Model manager | catalog and install manifests are schema/contract checked; traversal, symlink, unresolved license, file corruption, size, and SHA-256 mismatch refuse execution |
| Credential isolation | native Windows round trip when the logon vault exists; vault absence fails closed; secret diagnostics are redacted and no plaintext fallback exists |
| Privacy/logs | metadata key/event bounds, secret/path redaction, record limits, total-file rotation, persisted disable switch |
| Crash/process tree | generated contained partial cleanup only; queue restart coverage from Milestone 1; Windows descendant kill-on-close coverage from Milestone 2 |
| Release integrity | exact filename/size/SHA-256, symlink rejection, Authenticode required, automatic update forbidden, unsigned/corrupt fixture refused |
| Installer | config requires current-user NSIS/zlib; protected workflow signs executable then installer and tests silent install/uninstall on a separate fresh runner |
| Supply chain | locked dependency reconciliation, deterministic CycloneDX SBOM/notices, 17 schemas, npm high/critical gate, RustSec audit with two named expiring exceptions |
| Performance | startup below 5 s, queue recovery below 2 s, 64 MiB SHA-256 below 3 s, UI interaction budget below 100 ms |
| Offline E2E | all default suites use local deterministic fixtures, contain no paid API/model download, and verify no updater/model weight is bundled |
