# ADR 0003: Registry-backed content-addressed stage cache

- Status: Accepted
- Date: 2026-08-01

## Context

Long media stages must resume safely. A file may exist while incomplete, corrupt, produced by different settings, or left by an interrupted process.

## Decision

Register every artifact with project ID, type, normalized relative path, SHA-256, byte size, timestamp, producer stage, and metadata. Reuse requires a completed stage run with an exact project/stage/scope match. Its cache key covers a domain prefix, big-endian schema version, length-prefixed input/config hashes, engine/version, model version, and recursively key-sorted metadata JSON. Every registered output must verify by SHA-256 and size. Writes use temporary files and atomic promotion where supported.

## Consequences

Resume and targeted invalidation are explainable and safe, at the cost of hashing and registry bookkeeping. File existence alone never skips work. An integrity failure invalidates the stage. Source assets are immutable; invalid artifacts may be garbage-collected only after reference checks.
