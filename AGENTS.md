# VietDub Studio contributor guide

## Product and architecture

VietDub Studio is a privacy-conscious Windows-first desktop workflow for converting Chinese-language video into reviewed Vietnamese subtitles and dubbed audio. The desktop is Tauri 2 with a strict React/TypeScript UI. Rust owns projects, SQLite, jobs, artifacts, subprocesses, and all FFmpeg/ffprobe/yt-dlp/Python launches. Python 3.11 workers communicate only through versioned JSON Lines on stdin/stdout.

## Standard commands

- Install: run `corepack enable`, `pnpm install --frozen-lockfile`, and `python -m pip install -r requirements-dev.txt`
- Develop: `pnpm dev`
- Build: `pnpm build`
- Lint: `pnpm lint`
- Typecheck: `pnpm typecheck`
- All tests: `pnpm test`
- Suites: `pnpm test:rust`, `pnpm test:web`, `pnpm test:python`, `pnpm test:integration`

## Engineering rules

- Use English identifiers and technical documentation; user-facing copy is Vietnamese and must go through `src/lib/i18n.ts`.
- Rust modules and Python files use `snake_case`; Rust/TypeScript types and React components use `PascalCase`; TypeScript values use `camelCase`.
- Keep provider contracts in domain-facing modules. Concrete engines depend on those contracts; business logic must not import a specific cloud provider.
- Pin direct dependencies and commit lockfiles. Do not add GPL, AGPL, SSPL, or source-available runtime dependencies without written approval in `LICENSE_POLICY.md`.
- Never copy source, prompts, directory structures, or UI layouts from external video-translation projects. Implement from public specifications and general engineering principles only.
- Never use a shell to launch media tools or workers. Validate argument data, URLs, and canonical paths; keep artifacts inside their project; redact secrets and sensitive paths.
- Store secrets only in the OS credential store. Do not put secrets in SQLite, project JSON, logs, fixtures, or environment files committed to Git.

## Definition of Done

A change is done only when relevant build, lint, typecheck, and tests pass; schemas/migrations/docs are updated; failures are surfaced safely; no prior milestone regresses; and acceptance criteria are recorded. Fakes are test-only.

## Evolution rules

- Add migrations as immutable, zero-padded files in `apps/desktop/src-tauri/migrations/`; never edit a released migration. Register and test the next version in the migration runner.
- Add a provider by implementing its existing contract, keeping credentials behind the secret-store boundary, declaring data sent off-device, adding contract tests, and documenting license/model metadata.
- Outside the MVP: voice cloning, lip-sync, OCR, face swap, generative logo removal, DRM bypass, channel crawling, livestream, mobile, cloud rendering, team collaboration, and a plugin marketplace.

See `docs/` for architecture, decisions, threat model, roadmap, and test details.
