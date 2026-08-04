# Dependency inventory

Milestone 2 direct dependencies are declared and pinned in Cargo and pnpm lockfiles or `requirements-dev.txt`. Final SPDX/license verification remains a release-hardening gate.

| Ecosystem | Component | Purpose | Expected license |
| --- | --- | --- | --- |
| Rust | Tauri 2 | desktop host | Apache-2.0/MIT |
| Rust | Tokio, tokio-util, futures-util | async worker lifecycle | MIT |
| Rust | serde, serde_json, uuid | protocol data | MIT/Apache-2.0 |
| Rust | jsonschema | JSON Schema validation | MIT |
| Rust | rusqlite | SQLite migrations | MIT |
| Rust | sha2 | artifact and cache SHA-256 | MIT/Apache-2.0 |
| Rust | url 2.5.8 | strict HTTPS/authority/IDNA URL parsing | MIT/Apache-2.0 |
| Rust/Windows | windows-sys 0.61.2 | Windows Job Object, Credential Manager, and Authenticode verification | MIT/Apache-2.0 |
| Rust | thiserror | typed errors | MIT/Apache-2.0 |
| Web | React, React DOM | UI | MIT |
| Web | Vite, TypeScript, ESLint, Vitest, Testing Library | build and tests | permissive; verify lockfile |
| Python | jsonschema | contract validation | MIT |
| Python | faster-whisper 1.2.1 | local speech recognition runtime | MIT |
| Python | sentencepiece 0.2.1 | OPUS-MT source and target tokenization | Apache-2.0 |
| Python | pytest, ruff, mypy | test/lint/typecheck | permissive; verify pinned release |

Milestone 8 reviewed and minimally upgraded existing pinned dependencies: Serde 1.0.220 and time 0.3.47 (MIT/Apache-2.0) resolve the current RustSec `time` advisory; Vite 7.3.5 and Vitest 3.2.6 (MIT) resolve all npm high/critical findings. No new application package was added. `cargo-audit 0.22.2` is an external release tool installed ephemerally under its [MIT/Apache-2.0 upstream license](https://github.com/rustsec/rustsec/blob/main/cargo-audit/README.md).

The Windows installer is produced by NSIS in current-user mode with zlib compression. NSIS core is zlib/libpng licensed; using zlib avoids the optional LZMA module's separate CPL terms ([official NSIS license appendix](https://nsis.sourceforge.io/Docs/AppendixI.html)). No NSIS binary is committed to the repository.

Only model metadata ships. [Faster Whisper Large v3](https://huggingface.co/Systran/faster-whisper-large-v3) is approved for user-provided local use, not redistribution; its converted model card declares MIT and the original [OpenAI Whisper repository](https://github.com/openai/whisper) licenses code and model weights under MIT. FunASR Paraformer remains `UNRESOLVED-MODEL-LICENSE`, is not approved for local use or distribution, and therefore fails closed. No model weight is present in the SBOM or installer.

[SentencePiece 0.2.1](https://github.com/google/sentencepiece/tree/v0.2.1) is pinned for OPUS-MT tokenization under Apache-2.0. Its Windows wheel omits the Core Metadata license fields, so the release audit contains an exact `(name, version)` override backed by the upstream license review; unknown versions still fail closed. [Helsinki-NLP OPUS-MT Chinese-to-Vietnamese](https://huggingface.co/Helsinki-NLP/opus-mt-zh-vi) is approved only for explicit user-provided local use under Apache-2.0 and is not redistributed by VietDub Studio. The runtime manifest verifies the locally converted CTranslate2 files before execution.

[MeloTTS Vietnamese](https://huggingface.co/nmcuong/MeloTTS-Vietnamese) checkpoint `G_463000` and its MIT implementation at commit `235871bcec5450c3bbc179c2247cd2b243a43897` are approved only for explicit user-provided local use. The model, the MIT CharsiuG2P pronunciation resources, PhoBERT, and the local Python environment are not redistributed by VietDub Studio. Runtime manifests verify the checkpoint, configuration, and Vietnamese pronunciation dictionary before execution; translated text remains on-device.

`url` and `windows-sys` were already locked transitively through Tauri/Tokio; declaring them directly adds no new package source. Their local crate manifests declare `MIT OR Apache-2.0`; their Rust versions are below the workspace minimum. `url 2.5.8` uses the patched IDNA line recommended by RUSTSEC-2024-0421. No matching RustSec advisory was identified for either selected version during the 2026-08-01 review.

No FFmpeg, ffprobe, yt-dlp, model, font, cloud SDK, third-party media, or production fake provider is bundled in Milestone 2. FFmpeg licensing depends on the selected build and remains an external-manifest approval before distribution.

Milestone 6 adds no package or lockfile dependency. The approved `energy-mask-v1` separation engine is clean-room bundled project source, uses only the Python standard library plus the existing worker protocol/schema boundary, has no model files, performs no network access, and requires no install. Demucs was reviewed but not approved or included: its official repository declares the code MIT while the license/redistribution status of pretrained weights is not clearly resolved upstream.

The committed CycloneDX SBOM and notices are generated from the release environment. Current advisory disposition and expiry dates are in `docs/release/security-exceptions.md`.
