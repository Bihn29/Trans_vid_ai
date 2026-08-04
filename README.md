# VietDub Studio

VietDub Studio is a clean-room, Windows-first desktop application for producing reviewed Vietnamese subtitles and dubbed audio from Chinese-language video. Milestones 0-7 implement the guarded worker protocol, project/job/artifact foundation, media boundaries, ASR/transcript, translation, TTS, audio mixing, and typed composition. Milestone 8 adds release hardening, verified model metadata, native credential storage, privacy logs, recovery, SBOM/notices, and an Authenticode-gated NSIS release workflow.

## Prerequisites

- Windows 10/11 with WebView2 Runtime and the Visual Studio C++ build tools (the installer never downloads WebView2)
- Rust stable with Cargo
- Node.js 22.12 or newer (but below 23) and Corepack
- Python 3.11

## Local setup

```powershell
corepack enable
pnpm install --frozen-lockfile
python -m venv .venv
.\.venv\Scripts\Activate.ps1
python -m pip install -r requirements-dev.txt
```

Run the desktop application:

```powershell
pnpm dev
```

Run Rust/Tauri commands from a terminal opened after the Visual Studio C++ build tools are installed. If MSVC is not discovered automatically, use the Visual Studio 2022 Developer PowerShell.

Run all checks:

```powershell
pnpm lint
pnpm typecheck
pnpm build
pnpm test
pnpm schemas:check
pnpm release:audit
```

Individual suites are available as `pnpm test:rust`, `pnpm test:web`, `pnpm test:python`, and `pnpm test:integration`.

## Repository map

- `apps/desktop/`: React/Vite UI and Tauri Rust host
- `workers/`: isolated Python worker protocol and deterministic echo worker
- `schemas/`: versioned cross-language JSON contracts
- `docs/`: architecture, decisions, threat model, roadmap, and testing strategy
- `resources/`: verified binary/model manifest placeholders; no binary is downloaded automatically
- `tests/`: cross-process integration and security tests

Project state remains in SQLite and binary artifacts remain under isolated `projects/<uuid>/` roots. Local import is exposed through typed Tauri IPC. FFmpeg/ffprobe adapters require an absolute canonical executable plus a reviewed SHA-256; no external tool is bundled or downloaded automatically. Remote site support is a guarded adapter contract only, so local import remains usable without a downloader.

The release installer contains the desktop application and approved metadata manifests only. It contains no FFmpeg, ffprobe, yt-dlp, model weight, credential, private media, automatic updater, or WebView2 downloader. Production use of an external engine still requires its separately reviewed manifest and verified local installation.

The intended pipeline and milestone status are documented in [the implementation plan](docs/roadmap/implementation-plan.md). Security issues should follow [SECURITY.md](SECURITY.md).

## Privacy and clean-room policy

Telemetry is off by default. Production code must never upload media to a VietDub-operated service. Only a provider explicitly selected by the user may receive the minimum required audio or text. This project does not copy code or UI from public video translation projects.
