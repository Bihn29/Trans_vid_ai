# 0008. Provider-neutral TTS and duration fitting

- **Status:** Approved
- **Deciders:** Antigravity Team
- **Date:** 2026-08-01

## Context

Milestone 5 must synthesize reviewed Vietnamese translations without binding project state to a particular voice service. Each segment needs independent recovery and cache reuse, while voice or translation changes must invalidate only dependent work. Provider audio and metadata are untrusted, and sending text to a cloud voice requires explicit disclosure and consent. Voice cloning is outside the MVP.

## Decision

1. **Rust owns durable orchestration.** Voice assignments and per-segment TTS runs are persisted in SQLite. Assignment precedence is segment, then speaker, then project. A run records attempts, status, cache identity, artifact, measured duration, fitting rate, and warning code.
2. **The provider boundary remains neutral.** Domain types expose a voice catalog and assignment without importing a concrete SDK. Python providers implement one `TtsProvider` contract. The first approved adapter is an OpenAI-compatible HTTPS speech endpoint using fixed PCM WAV output and the approved built-in `alloy` and `nova` voices.
3. **Credentials stay behind the existing store boundary.** Rust retrieves a `SecretString` immediately before worker execution. Only a credential reference may be persisted. The production `UnavailableCredentialStore` fails closed until the Milestone 8 OS credential-store adapter is installed.
4. **Cache identity is per segment.** The identity hashes the translated-text hash, provider, voice, model, and requested speed. A completed run is reusable only when its registered artifact still passes project ownership, path containment, SHA-256, and size verification.
5. **Provider output is verified twice.** Python validates a bounded PCM WAV response before writing a request-unique contained file. Rust independently verifies the descriptor, path, hash, size, PCM WAV structure, sample metadata, and measured duration before registry insertion and segment update.
6. **Fitting is policy, not destructive audio processing.** Rust computes `source_duration / target_duration`, clamps the proposed playback rate to 0.85–1.20, and emits `SHORTEN_TRANSLATION` or `EXCESSIVE_SLOWDOWN` outside that range. Audio time-stretching and mixing remain Milestone 6 work.
7. **Failure and recovery are segment-local.** Successful segments remain completed if another segment fails. Restart returns only running segment records to pending. Retry creates the normal new Job/StageRun attempt and reuses verified completed segment artifacts by identity.
8. **Previews are independent artifacts.** A preview uses the same provider and verification boundary but registers as `preview` and never replaces the segment's selected TTS artifact.
9. **No custom voice material is accepted.** The request schema contains only an approved catalog `voice_id`; it has no sample upload, consent-recording, speaker embedding, or cloning field. Deterministic TTS exists only under test fixtures.

## Consequences

- Two speakers can route to distinct approved voices while project, speaker, and segment overrides remain deterministic.
- Cloud transfer is visible and requires explicit consent; no credential or authorization value enters SQLite, artifacts, metrics, or safe errors.
- Corrupt cache entries are regenerated instead of silently reused.
- The first adapter uses only the standard library and existing protocol/schema dependencies, so Milestone 5 adds no package or lockfile dependency.
- Production cloud synthesis remains unavailable until a real OS credential-store implementation is supplied. Audio separation, time stretching, mixing, and composition remain later milestones.
