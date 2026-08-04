# 0009. Local separation and typed audio mix plan

- **Status:** Approved
- **Deciders:** Antigravity Team
- **Date:** 2026-08-02

## Context

Milestone 6 needs source separation, a safe fallback, timeline assembly, gain/duck/fade controls, normalization, limiting, and mixed WAV export. It must remain deterministic and offline-testable, preserve successful TTS artifacts, and avoid exposing raw FFmpeg filters. A reviewed third-party model could be used only if its code, weights, install path, checksums, and redistribution terms were all approved.

The official Demucs repository declares its code MIT, but the upstream question about the pretrained model license remains unresolved. Shipping or silently installing those weights would therefore violate the project's model-license policy. No Demucs code, package, or model is included in this milestone.

## Decision

1. **Use a provider-neutral separation boundary.** `SeparationEngineDescriptor` and the JSONL worker contract are independent of a concrete engine. Every worker result remains untrusted until Rust verifies project ownership, relative containment, SHA-256, size, PCM structure, sample rate, and stem alignment.
2. **Approve the built-in `energy-mask-v1` engine.** It is clean-room project source, has no model or package dependency, sends no data off-device, requires no install or consent, and never downloads. Its committed manifest states each of those facts explicitly. It splits mono PCM frames by relative energy and exactly reconstructs the input when the two stems are summed.
3. **Fail over to attenuation, not fabricated success.** If the worker cannot start, fails, or produces an invalid response, Rust derives an explicit `fallback_attenuation` background at 25% of the verified source and preserves a separate original-vocals lane. Cancellation never triggers fallback and produces no terminal artifact.
4. **Use a typed Rust DSP plan.** Rust reads verified PCM16 mono WAV artifacts and builds lanes from background, optional original voice, optional music, and segment TTS. TTS is linearly fitted to its segment interval; fades and ducking are sample-indexed. The plan is serialized and SHA-256 hashed. No user-controlled filter string or raw FFmpeg option exists.
5. **Normalize and limit before export.** The mixer applies bounded RMS normalization, then a configurable peak limiter. It exports a new request-unique mixed PCM WAV and records duration, peak, RMS, limited sample count, post-limit clipping count, separation mode, and timeline hash.
6. **Treat TTS as immutable input.** Mixing opens TTS artifacts read-only after registry verification and writes only under `audio/mixed`. Failure or cancellation cannot update a segment or mutate/unregister a successful TTS artifact.
7. **Persist only user settings and normal stage state.** Project mix controls are stored in migration 0006. Existing StageRun/Job persistence owns retry, recovery, cancellation, cache metadata, and output artifact IDs. Changing settings invalidates `MIX_AUDIO` and the project tail.

## Consequences

- Milestone 6 has no new dependency, lockfile change, cloud transfer, silent install, or model-license exposure.
- The typed plan is deterministic and safer than interpolated media filter strings, but the in-process DSP currently supports bounded mono PCM16 WAV only.
- `energy-mask-v1` is a conservative baseline, not a neural source-separation quality claim. The visible attenuation fallback protects workflow continuity when it cannot be used.
- A future engine can implement the same contract only after its binary/model manifest, license, consent, checksums, security boundary, and deterministic tests are approved.
- Subtitle composition, video rendering, and raw FFmpeg filters remain Milestone 7 work.
