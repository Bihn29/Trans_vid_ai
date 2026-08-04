# Pipeline architecture

## Stages

The workflow is a persisted directed sequence:

```text
IMPORT -> PROBE -> NORMALIZE -> EXTRACT_AUDIO -> SEPARATE_AUDIO
 -> TRANSCRIBE -> SEGMENT -> TRANSCRIPT_REVIEW -> TRANSLATE
 -> TRANSLATION_REVIEW -> VOICE_ASSIGNMENT -> VOICE_PREVIEW
 -> SYNTHESIZE -> FIT_DURATION -> MIX_AUDIO -> COMPOSE_VIDEO
 -> QUALITY_CHECK -> RENDER -> COMPLETE
```

Review stages stop the current run with `review_required`; they do not reserve a thread, process, or concurrency slot. User confirmation enqueues the following stage.

## Milestone 2 media stages

- `IMPORT` copies local media without a process or network dependency. Remote downloader adapters are separate and promote only a contained generated staging file.
- `PROBE` invokes ffprobe with fixed flags, validates bounded JSON, and persists a versioned metadata artifact.
- `NORMALIZE` creates a bounded 1280-wide H.264/AAC proxy through fixed FFmpeg arguments.
- `EXTRACT_AUDIO` creates mono 16 kHz PCM WAV through fixed FFmpeg arguments.

An adapter success is not a stage success until the child exits successfully and a non-empty contained output verifies and is registered. Tool paths, user-provided raw flags, output templates, and filter strings are never accepted over IPC.

## Milestone 3 ASR and transcript stages

- `TRANSCRIBE` invokes the ASR worker (`workers/asr/main.py`) through the Rust `AsrPipelineService` and `WorkerManager`. The input is a verified mono 16 kHz project artifact. FunASR is preferred and a separately consented faster-whisper installation is the fallback.
- Model execution requires a persisted consent snapshot and a local `vietdub-model.json` whose provider/license match that consent. Every declared model file is contained, size-checked, and SHA-256 verified before worker creation. No production path downloads a model.
- The worker runs with the canonical project directory as its working directory, accepts only contained relative audio/output paths, writes a request-unique transcript file, flushes it, and reports its SHA-256 and size. Rust independently verifies those values before Artifact Registry registration.
- `SEGMENT` validates the transcript schema, normalizes timing, and persists segment records. Full runs atomically replace the transcript; regional reruns invalidate and replace only overlapping segments while preserving unaffected records.
- `TRANSCRIPT_REVIEW` is created directly as a persisted `review_required` checkpoint and never claims a runtime slot. Users can view QC warnings and edit timestamps, speaker, enabled state, or source text; split/merge invariants and project ownership are enforced. Approval completes the latest checkpoint. Editing performs persisted dependency invalidation without re-running unchanged ASR.

## Stage run contract

Every run records `stage_id`, `project_id`, `status`, `progress`, input/config hashes, engine/model versions, timestamps, safe failure data, and output artifact IDs. Status is one of `pending`, `queued`, `running`, `review_required`, `completed`, `failed`, `cancelled`, or `invalidated`.

A stage can be reused only when its cache key and all registered output artifacts verify. File existence alone is insufficient.

The persisted cache format is fixed as follows:

```text
SHA-256(
  "VIETDUB_STAGE_CACHE\\0" ||
  schema_version:u32-big-endian ||
  length:u64-big-endian || input_hash ||
  length:u64-big-endian || config_hash ||
  length:u64-big-endian || engine_name ||
  length:u64-big-endian || engine_version ||
  length:u64-big-endian || model_version ||
  length:u64-big-endian || canonical_metadata_json
)
```

Metadata object keys are sorted recursively; array order is preserved. Input and configuration hashes are lowercase SHA-256 hex. A golden unit fixture prevents serialization drift. Cache lookup additionally requires the same project, stage name, and stage scope. A corrupt or missing output invalidates the completed run instead of returning a hit.

## Invalidation rules

| Change | Preserve | Invalidate |
| --- | --- | --- |
| Source transcript segment | media and ASR for other segments | translation/TTS for the segment, mix, compose, render |
| Vietnamese text | ASR and source transcript | TTS for the segment, mix, compose, render |
| Voice assignment | ASR and translation | affected TTS, mix, compose, render |
| Subtitle style | ASR, translation, TTS, audio mix | compose and render |
| Crop, watermark, resolution | transcript, translation, TTS, audio | compose and render |

Invalidation is transactional and dependency-driven. It never deletes the immutable source. Obsolete artifacts remain eligible for garbage collection only when no valid registry entry references them.

## Queue behavior

The persistent queue claims by descending priority and stable enqueue order, with configurable non-zero concurrency across projects. Pause prevents new work while preserving resumable state. A running pause/cancel sets a persisted request and signals only that job's cancellation token; the provider acknowledges the interruption before the terminal transition. Retry creates a new Job and StageRun attempt while preserving failure history and cache identity. Review completion releases the in-memory slot. Timeouts, cancellation tokens, bounded output capture, and checked exit status apply to every child process.

## Worker exchange

Rust validates the request schema before spawn, writes exactly one JSON object plus newline, closes stdin, and validates each returned line. Progress may repeat; exactly one terminal `completed` or `failed` event is accepted. A terminal event is not success until the child exits successfully. Oversized, malformed, mismatched, or post-terminal events are protocol errors.

## Milestone 5 TTS stages

- `VOICE_ASSIGNMENT` persists approved catalog selections at project, speaker, or segment scope. Resolution is deterministic: segment overrides speaker, and speaker overrides project. A change transactionally invalidates affected `SYNTHESIZE`/`FIT_DURATION` work and the project tail; it never invalidates transcript or translation.
- `VOICE_PREVIEW` sends one reviewed translated segment through the same provider, credential, containment, and WAV-verification boundary as synthesis. The registered preview artifact is independent and cannot replace segment audio.
- `SYNTHESIZE` creates one durable record per enabled translated segment. Cache identity includes translation hash, provider, voice, model, and speed. Completed records survive another segment's failure or application restart and are reused only while their Artifact Registry entries verify.
- The approved cloud adapter requests fixed PCM WAV, uses a bounded response and retry policy, and accepts only catalog voice IDs. Cloud consent is mandatory. Deterministic audio generation is test-only.
- Rust measures PCM WAV duration after independent hash/size/path verification. It proposes a playback rate in the safe 0.85–1.20 range and raises a human warning outside that range. Actual time stretching, timeline assembly, and mixing are intentionally deferred to Milestone 6.

## Milestone 6 audio stages

- `SEPARATE_AUDIO` invokes the provider-neutral separation worker with one verified `original_audio` artifact. The approved built-in `energy-mask-v1` engine is local, model-free, download-free, and deterministic. Both returned PCM stems must match sample rate and sample count before registration.
- Worker start/failure/invalid output activates a Rust-owned `fallback_attenuation` path. It creates a 25% background and a separate original-vocals lane from the verified source. Cancellation is never converted into fallback success.
- `MIX_AUDIO` builds a typed sample-index plan from background, optional original vocals, optional music, and each enabled segment's verified TTS artifact. Segment start/end timestamps define placement; deterministic linear fitting, bounded fades, and ducking apply without a raw filter string.
- RMS normalization is bounded to 0.25–4.0 gain, followed by a configurable hard peak limiter. The new PCM16 mono WAV is accepted only when duration differs from the background by at most one millisecond and post-limit clipping is zero.
- Audio artifact inputs are immutable. The stage writes only a request-unique `audio/mixed` output, records QC and a deterministic timeline hash in Artifact Registry metadata, and leaves all successful TTS artifacts unchanged.

## Milestone 7 composer stages

- `COMPOSE_VIDEO` validates a persisted typed config for trim, crop, resize/aspect/padding, blur, flip, speed, subtitle mode, text, logo/watermark, cover regions, and draft/final preset. No command accepts raw FFmpeg options.
- SRT cues are derived from enabled translated segments and adjusted for trim/speed. Soft subtitles map a `mov_text` stream; burned subtitles reference a generated contained SRT file. User text is supplied through generated text files and never interpolated into the filter graph.
- Render identity includes typed config and verified source, mixed-audio, overlay, and subtitle dependency hashes. Composition changes invalidate `COMPOSE_VIDEO`, `QUALITY_CHECK`, `RENDER`, and `COMPLETE` through the existing engine.
- `RENDER` runs an approved checksum-verified FFmpeg executable with discrete arguments and the canonical project root as working directory. It writes a request-unique partial MP4, promotes only a non-empty regular output, then registers it only after ffprobe verifies video/audio streams, dimensions, and duration tolerance.
- Cancellation removes partial and race-completed outputs before the queue acknowledges the job. Retry remains a new persisted Job/StageRun attempt. Verified source and mixed-audio inputs are opened read-only and never replaced.
- SRT/WAV export verifies the registry entry again and uses a canonical destination parent plus create-new semantics. Existing files cannot be overwritten.

## Milestone 8 operational hardening

- Worker creation now requires both persisted consent and a catalog-approved, user-provided model installation. The installation manifest and every contained model file are independently size/SHA-256 verified; an unresolved license or corrupt file refuses execution.
- Credentials cross only the native Windows Credential Manager adapter. IPC can save, delete, or report availability, but can never read a secret value back into the UI.
- Metadata diagnostics use restricted event codes/keys, redaction, fixed record bounds, configurable rotation, and a persisted disable switch. Media text, absolute paths, credentials, command lines, and worker output are not logged.
- Startup records a runtime session, recovers interrupted queue state, and removes only generated contained partials. Existing Job Object kill-on-close covers descendant processes after a host crash.
- Release metadata is outside pipeline cache identity. A signed installer must match the release manifest filename, byte size, SHA-256, and Authenticode policy; no background updater or download stage exists.
