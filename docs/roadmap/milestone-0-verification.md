# Milestone 0 verification

- Date: 2026-08-01
- Platform: Windows, Node 22.23.2, pnpm 9.15.4, Python 3.11.9, Rust/Cargo 1.97.1
- Scope: repository foundation only

## Acceptance evidence

- `pnpm install --frozen-lockfile`: passed; pnpm lockfile was current.
- `python -m pip install -r requirements-dev.txt`: passed in the workspace virtual environment.
- `pnpm lint`: passed ESLint, rustfmt check, and Ruff.
- `pnpm typecheck`: passed strict TypeScript and strict mypy for eight Python source files.
- `pnpm build`: passed Vite production build and locked Rust workspace build.
- `pnpm test:web`: 3 passed.
- `pnpm test:rust`: 10 passed, including real Rust-to-Python exchange, failure, timeout, cancellation, and message-limit cases.
- `pnpm test:python`: 11 passed.
- `pnpm test:integration`: 1 passed using a real echo worker subprocess.
- `pnpm dev`: Vite listened on `127.0.0.1:1420`, Cargo launched `target/debug/vietdub-studio.exe`, and the `VietDub Studio` window was observed. The verification harness then intentionally terminated only its spawned dev process tree.

## Security evidence

Source scans found no `shell=true`, `eval`, `exec`, private-key block, bearer token, or API-key-shaped literal. Production child-process creation is centralized in the Rust worker client and uses `tokio::process::Command` with discrete arguments. Request/response schemas, relative-path checks, request correlation, bounded JSON Lines/stderr, safe failures, checked exit status, timeout, cancellation, and redaction have tests.

## Residual risks

Windows process-tree termination needs a Job Object before workers may spawn descendants. URL, downloader, project isolation, real media tools, secret storage, log rotation, and binary/model verification belong to later milestones. Milestone 0 ships no AI model, media binary, cloud adapter, installer, or production fake provider.

