# ADR 0005: Guarded media import and external-tool boundary

- Status: Accepted
- Date: 2026-08-01

## Context

Milestone 2 introduces untrusted local media, remote URLs, native codec tools, and potentially descendant processes. Local import must remain useful without network tooling. Passing raw options or site filenames into tool/project paths would break the containment and reproducibility decisions from Milestones 0 and 1.

## Decision

Keep local import independent and first-class. Copy a regular local source with a streaming size cap to a generated project path, promote before registry insertion, mark it read-only, and attach only one immutable source artifact.

Represent remote support as per-site downloader contracts. The network policy parses HTTPS URLs, resolves the exact allowlisted host, rejects every non-public address, and returns socket addresses that the adapter must use. Every redirect repeats the same checks and cannot change site family. Downloaded names are discarded; providers stage to generated project-temp names before import promotion. Milestone 2 ships no concrete downloader or automatic network access.

Centralize ffprobe/FFmpeg execution in a generic supervisor. Approved executables are canonical files with a reviewed SHA-256 checked before every spawn. Commands use fixed argument arrays, bounded output, timeout/cancellation, checked exit status, required-output verification, kill-and-wait, and a Windows kill-on-close Job Object. Raw flags are not IPC data.

## Consequences

Local media works offline and cannot fail because downloader health changed. A future concrete site adapter must use the guarded resolved addresses rather than resolving again, and must add its license/terms/security review. FFmpeg remains externally configured until an exact redistributable build is approved. Codec parsing is lifecycle-bounded but not OS-sandboxed; stronger isolation remains release hardening.
