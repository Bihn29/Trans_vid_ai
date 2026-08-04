# ADR 0002: Versioned JSON Lines worker protocol

- Status: Accepted
- Date: 2026-08-01

## Context

AI engines have conflicting Python dependencies and can fail or consume large resources. A local HTTP server adds ports, authentication, lifecycle, and SSRF surface without benefit for a single desktop owner.

## Decision

Run each engine family as an independent Python 3.11 process. Rust sends one JSON Lines request on stdin and receives bounded progress plus exactly one terminal event on stdout. Both directions use committed JSON Schemas, protocol version 1, UUID request correlation, timeout, cancellation, checked exit code, and bounded redacted stderr. Rust launches a fixed executable with an argument array and never a shell.

## Consequences

Workers remain replaceable and isolated; deterministic workers enable offline tests. The protocol cannot carry binary data, so artifacts use validated project-relative paths. Streaming large logs or model output on stdout is prohibited. Schema evolution requires compatible version handling or a protocol-version increment.

