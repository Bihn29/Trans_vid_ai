# Dependency and asset license policy

VietDub Studio is clean-room software. External code, prompts, component structures, UI layouts, model files, and media are not copied into this repository unless their origin and license are explicitly reviewed and recorded.

## Allowed by default

Permissive runtime and development dependencies under MIT, BSD-2-Clause, BSD-3-Clause, Apache-2.0, ISC, CC0, or comparable terms may be proposed. Every direct dependency must be pinned, represented in a lockfile, and entered in the dependency inventory before release.

## Approval required

GPL, AGPL, SSPL, BUSL, Elastic License, non-commercial, research-only, source-available, custom model licenses, and dependencies with unclear redistribution terms require written approval. Approval must describe distribution obligations and whether the item can ship in the installer.

FFmpeg codec and build licensing varies by configuration. yt-dlp and any separation/ASR/TTS model require a separate distribution decision. Milestone 2 provides command/provider contracts but does not bundle or auto-download FFmpeg, ffprobe, yt-dlp, media, or models. Remote services remain subject to their terms and must be opt-in.

Milestone 6 approves only the clean-room built-in `energy-mask-v1` DSP engine recorded in `resources/manifests/separation-energy-mask.json`. It has no external weights or runtime package. Demucs code and pretrained weights are not included or approved because upstream model-weight licensing is not sufficiently clear for distribution.

## Required metadata

For every shipped library, binary, model, font, or media asset record: name, version, source, license/SPDX identifier, copyright notice, distribution mode, and checksum where applicable. Release hardening will generate an SBOM and notices from the committed lockfiles and approved manifests.

## Milestone 8 release decisions

- The installer ships no FFmpeg/ffprobe/yt-dlp binary and no model weight. Model manifests are informational approvals; `approved_for_distribution` is always false.
- Faster Whisper Large v3 may be verified for explicit user-provided local use under its recorded MIT metadata. It may not be redistributed by VietDub Studio. FunASR Paraformer remains blocked because its model license is unresolved.
- Helsinki-NLP OPUS-MT Chinese-to-Vietnamese may be verified for explicit user-provided local use under Apache-2.0. It may not be redistributed by VietDub Studio. SentencePiece 0.2.1 is approved as its Apache-2.0 tokenizer runtime; the release audit pins the license override to that exact wheel version because its Windows metadata omits the license fields.
- NSIS is approved only with zlib compression under its permissive core licensing. Optional compression/plugin modules with additional terms are not selected.
- Every release regenerates the CycloneDX SBOM and notices, rejects missing/unknown/banned licenses, runs npm and RustSec advisory gates, and records any temporary advisory exception by exact ID, reachability, owner, and expiry.
