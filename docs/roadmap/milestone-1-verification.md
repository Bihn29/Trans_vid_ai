# Milestone 1 verification

## 1. Scope

- Date: 2026-08-01
- Platform: Windows, Node 22.23.2, pnpm 9.15.4, Python 3.11.9, Rust/Cargo 1.97.1
- Implemented only project, artifact, StageRun, Job, queue, recovery, cache, and invalidation foundations.
- No Milestone 2 media-import work or real media/AI integration is present.

## 2. Pre-change baseline

The verified Milestone 0 workspace passed lint, strict type checks, production builds, Web tests, Rust tests, Python tests, and the real echo-worker integration test before Milestone 1 code was changed.

## 3. Buildable increments

1. Add migration 2 and domain/persistence records.
2. Add isolated project CRUD and artifact integrity registry.
3. Add cache identity and targeted invalidation.
4. Add persistent queue state machines and restart recovery.
5. Add typed Tauri project commands, tests, and acceptance documentation.

Each increment compiled before the next was added.

## 4. Persistence model

Migration 2 adds strict `projects`, `artifacts`, `stage_runs`, and `jobs` tables with foreign keys, state checks, progress/attempt constraints, and claim/project/scope indexes. The migration runner upgrades an existing version-1 database transactionally and rejects newer unknown schemas.

## 5. Project and artifact foundation

Project CRUD writes SQLite as authority and a derived `project.json` snapshot. Every project gets a UUID directory and fixed subdirectory layout. Delete moves the directory to contained trash before deleting the row. Artifact registration accepts only a contained relative file, records SHA-256 and byte size, and reports verified, missing, or corrupt.

## 6. Stage, cache, and invalidation foundation

StageRun persists the full stage/scope/status/cache/version/attempt/timestamp/error/output record. Cache serialization is domain-separated, length-prefixed, recursively canonicalized, and protected by a golden digest. Reuse requires an exact project/stage/scope match and verified outputs. Segment/speaker/project invalidation is targeted and transactional; a running affected stage blocks invalidation.

## 7. Queue and state machines

The SQLite queue claims by priority and enqueue order with configurable concurrency. Pause, resume, cancel, complete, fail, review, and retry transitions persist state. Each running job owns a distinct cancellation token. Retry appends linked Job and StageRun attempts. Review releases its runtime slot.

## 8. Restart recovery

Startup scans persisted running jobs. Cancel requests become cancelled, pause requests become paused, and other interrupted jobs return to queued with `APP_RESTART_RECOVERY`. Tests cover queue recreation and closing/reopening the physical SQLite database.

## 9. Security evidence

Negative tests cover traversal, absolute/drive/alternate-separator paths, Windows device aliases, canonical symlink escape where OS privilege permits, artifact mutation/removal, invalidation races, and concurrent cancellation isolation. Tauri command failures expose stable codes only. Source scans verify that no prohibited media/AI tool or production deterministic provider was added.

## 10. Quality gates

- `pnpm lint`: passed ESLint, rustfmt check, and Ruff.
- `pnpm typecheck`: passed strict TypeScript and mypy for eight Python source files.
- `pnpm build`: passed Vite production build and locked Rust workspace build.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: passed.
- Web: 3 passed.
- Rust: 26 passed: 10 unit, 11 Milestone 1 integration/security, and 5 worker-protocol tests.
- Python: 11 passed.
- Real echo-worker subprocess integration: 1 passed.
- Forbidden-integration scan: no FFmpeg/ffprobe, yt-dlp, FunASR, cloud translation, TTS provider, or production deterministic provider in the Milestone 1 Rust scope.

## 11. Residual risk and stop condition

SQLite and filesystem operations require compensating actions because they cannot share an atomic transaction. The project snapshot is derived and recoverable from SQLite. Windows symlink creation may be unavailable to an unprivileged test account, although syntactic containment tests always run. Real child-process tree control remains a later prerequisite before spawning tools that create descendants.

Milestone 1 stops here. Milestone 2 requires explicit authorization.
