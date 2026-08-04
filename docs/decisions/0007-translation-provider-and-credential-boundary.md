# 0007. Translation provider and credential boundary

- **Status:** Approved
- **Deciders:** Antigravity Team
- **Date:** 2026-08-01

## Context

Milestone 4 adds local and cloud-capable translation without allowing provider details, untrusted model output, or credentials to become application business state. Translation can span many blocks, so a provider failure or application restart must not discard completed work. Cloud text transfer must remain explicit and visible.

## Decision

1. **Rust owns durable state.** Projects, glossary entries, locked proper names, block identity/status, attempts, partial results, jobs, and review checkpoints remain in SQLite. A block identity hashes source segments plus provider/model/language/glossary/name configuration.
2. **Credentials remain behind a store boundary.** Rust requests a `SecretString` through `CredentialStore` immediately before worker execution. Only the lookup reference is configuration; secret values are never written to SQLite, project files, diagnostics, or artifacts. The production default denies cloud execution until an OS credential-store adapter is configured.
3. **Provider-neutral Python contract.** Providers receive deterministic system/user prompts and return untrusted text. The OpenAI-compatible adapter formats the standard chat-completions request over validated HTTPS with redirects disabled. The local adapter is a transport contract and declares that data stays on-device.
4. **Two-sided strict validation.** Python accepts only one JSON object matching the versioned result schema, with the exact requested ID set, non-empty text, and unchanged locked names. Rust independently verifies artifact containment, size, SHA-256, JSON Schema, exact IDs, and locked names before an atomic block/segment commit.
5. **Bounded recovery.** A worker performs at most three attempts. Every block transition is persisted. Restart changes only `running` blocks back to `pending`; completed blocks remain complete. A job retry reuses a completed result only when block segment IDs and the full source/config identity match.
6. **Human and cloud checkpoints do not consume worker slots.** Cloud providers require explicit consent and are disclosed in the review UI. Translation completion creates a durable `TRANSLATION_REVIEW` checkpoint represented by a completed queue job.
7. **Test doubles stay outside production.** Deterministic providers are defined in Python unit tests or `tests/fixtures/translation_workers` only.

## Consequences

- Provider failures, retries, cancellations, and restarts preserve verified block progress.
- Cloud secrets have a narrow, testable lifetime and cannot silently enter persistence.
- Adding a provider requires an adapter conforming to the same strict output contract.
- A platform OS-keychain implementation and a concrete local translation runtime remain later packaging/integration work; neither is silently simulated in production.
- No new dependency is required for Milestone 4.
