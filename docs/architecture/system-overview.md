# System overview

## Goals and boundaries

VietDub Studio is a local desktop workflow. It preserves original media, records reproducible stage inputs and outputs, pauses at human review checkpoints, and resumes valid work after restart. Windows is the first release target, but domain and worker contracts avoid Windows-only behavior.

Milestone 0 proves the application shell and Rust-to-Python boundary. Milestone 1 implements project/job/artifact persistence. Milestone 2 implements immutable local import, guarded remote adapter contracts, ffprobe/FFmpeg command adapters, and a bounded external-process supervisor. AI engines and rendering begin in later milestones.

## Runtime components

1. **React/Vite UI** presents Vietnamese workflow screens and sends typed Tauri commands. It never launches a process or reads secrets directly.
2. **Tauri/Rust host** is the authority for lifecycle, paths, persistence, queueing, artifacts, process execution, and safe errors. Only this layer may start approved executables.
3. **SQLite** stores project metadata, queue state, stage runs, and artifact registry records. Later migrations add segments and speakers. Binary media stays in project directories. Versioned migrations are forward-only.
4. **Python workers** isolate engine families. A worker receives one schema-validated JSON Lines request, emits bounded progress events, and ends with one completed or failed event.
5. **External tools/providers** are adapters behind Rust or Python contracts. Cloud use is explicit and visible; local file workflows must remain available if network engines fail.

Milestone 2 does not bundle or auto-download FFmpeg, ffprobe, or yt-dlp. Tool adapters accept only an absolute canonical executable whose SHA-256 matches reviewed configuration. The generic supervisor uses argument arrays, bounded stdout/stderr, timeout/cancellation, checked exit status, kill-and-wait, and a Windows Job Object with kill-on-close.

Milestone 8 adds the release trust boundary. A metadata-only model catalog is bundled, while every model remains an explicit user-provided install verified by file hash and size. Provider secrets live only in Windows Credential Manager. Bounded metadata logs and runtime crash markers live in application data. Distribution is a manually initiated, current-user NSIS installer with no updater; the application executable and installer are independently Authenticode-signed and verified on a fresh Windows runner.

## Trust boundaries

```text
Untrusted file/URL/user text
          |
          v
React UI --typed IPC--> Rust host --validated JSONL--> Python worker
                            |                         |
                            +--arg arrays--> tools   +--> selected AI provider
                            |
                            +--> SQLite + project-scoped files
```

Every boundary validates structure, size, and ownership. The UI receives stable error codes and Vietnamese-safe messages, not command lines, tracebacks, credentials, or sensitive absolute paths.

## Project and artifact layout

Each project is rooted under `projects/<uuid>/` with `source`, `proxy`, `audio`, `subtitles`, `metadata`, `previews`, `renders`, `logs`, and `temp` subdirectories. Database paths are normalized relative paths. Absolute paths, traversal, alternate separators, Windows device aliases, and canonical symlink escapes are rejected. Source media is immutable. Artifact identity includes SHA-256, byte size, producer stage, and metadata. Deletion first moves the directory to project-local trash, then removes the database record.

Local source import copies a regular non-symlink `.mp4`, `.mov`, `.mkv`, or `.webm` into a create-new temporary file with a streaming byte cap, flushes it, atomically promotes a generated UUID filename, marks it read-only, registers integrity, and finally attaches it to the project. Remote adapters must stage to a generated `temp/` name; site-provided filenames never become project paths.

## Persistence and recovery

SQLite migrations are embedded and applied transactionally in ascending version order. Queue bootstrap converts interrupted `running` work to `paused`, `cancelled`, or `queued` according to persisted requests. A normal interrupted run is requeued with a recovery code; it is never marked successful. Cache reuse requires an exact project/stage/scope/cache match and verification of every registered output. Review checkpoints release workers and queue capacity.

Runtime-session rows distinguish normal shutdown from a process or OS crash. On the next start, interrupted rows are marked recovered, queue recovery runs, and cleanup visits only the canonical project root, skips symlinks, and removes only generated dot-prefixed partial files. Windows Job Objects close with the parent and terminate any supervised descendant tree.

## Dependency direction

UI features depend on typed UI services, never process implementations. Rust commands depend on domain services; infrastructure implements domain contracts. Provider-specific Python adapters normalize results before protocol output. Cross-language data is governed by `schemas/`, not duplicated informal types.
