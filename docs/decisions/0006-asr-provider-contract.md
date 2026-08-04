# 0006. ASR provider contract and transcript segmentation

- **Status:** Approved
- **Deciders:** Antigravity Team
- **Date:** 2026-08-01

## Context

Milestone 3 introduces Chinese ASR (Automatic Speech Recognition) and segment management to VietDub Studio. Speech recognition engines (such as FunASR and faster-whisper) have disparate Python APIs, data structures, confidence metrics, and segment boundaries. Furthermore, AI model binaries are multi-gigabyte downloads that must not be bundled with the core application or required for offline tests.

## Decision

1. **Normalized Provider Contract**: All ASR engine integration is hidden behind a strict `AsrProvider` Python protocol (`workers/asr/contract.py`). Concrete adapters (`FunAsrProvider`, `FasterWhisperProvider`) convert engine-specific outputs into standard `AsrSegment` records (`start_ms`, `end_ms`, `text`, `confidence`, `speaker_label`).

2. **Deterministic Test-Only Provider**: Default contract tests use local test doubles, and the cross-process fixture is stored under `tests/fixtures/asr_workers`. Production `workers/asr` contains no deterministic provider or model default.

3. **Explicit Model Consent and Local Integrity**: `WorkerManager.client_for_stage` checks consent for every required model, then validates the local install manifest and every declared file before process creation. The Python adapters receive verified local directories and never a downloadable model name.

4. **Verified Artifact Handshake**: Python writes and flushes a request-unique transcript below the project root. Rust checks containment, declared hash/size, JSON Schema, segment invariants, and Artifact Registry integrity before completing the ASR job.

5. **Python-Side Normalization and Rust-Side Invalidation**: Segment normalization and transcript quality checks occur in the worker. Transactional persistence, regional replacement, transcript review checkpoints, editor operations, and targeted downstream invalidation occur in Rust.

6. **Invariants and Governance**:
   - `end_ms > start_ms` is strictly enforced at every boundary.
   - Editing source text recomputes `source_hash` and invalidates translation/TTS/mix/compose/render stages without re-running ASR on unchanged segments.
   - Approved transcripts block automatic modifications until unapproved.

## Consequences

- Tests execute rapidly, deterministically, and offline.
- Adding a new ASR engine requires only implementing `AsrProvider`.
- Core application architecture remains independent of any cloud or local AI runtime library.
