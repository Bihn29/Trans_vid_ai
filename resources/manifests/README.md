# Verified resource manifests

Binary and model manifests must record version, approved HTTPS source, SHA-256, size, license, and explicit-install policy. Milestone 2 adds executable checksum enforcement and command adapters, but intentionally ships no FFmpeg, ffprobe, yt-dlp, external media, or model. A user- or installer-provided executable cannot enter `ApprovedTool` without an expected SHA-256 from separately reviewed metadata.

Milestone 3 defines `schemas/installed-model-manifest.schema.json`. Each local model directory must contain `vietdub-model.json`; Rust checks its identity, provider, license, HTTPS source, and every declared file's relative path, size, and SHA-256 immediately before worker creation. No model binaries are committed or downloaded by the application.

Milestone 8 bundles the metadata-only catalog under `models/` and validates it against `schemas/approved-model-manifest.schema.json`. Faster Whisper Large v3 is approved only for explicit user-provided local use and never for redistribution. FunASR Paraformer remains blocked because its model license is unresolved. Catalog approval, persisted user consent, and a verified installation are all required before worker creation.
