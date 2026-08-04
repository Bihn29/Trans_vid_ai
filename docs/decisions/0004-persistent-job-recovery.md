# ADR 0004: Persistent job attempts and restart recovery

- Status: Accepted
- Date: 2026-08-01

## Context

Desktop shutdown, crashes, and user pause/cancel requests can interrupt a stage. Treating an interrupted job as completed would corrupt pipeline history; restarting every project would discard valid work. Cancellation must also remain isolated across concurrent projects.

## Decision

Persist every Job and StageRun attempt in SQLite. Claim the next queued job transactionally by descending priority and stable enqueue order while enforcing configurable concurrency. Give each claimed job its own cancellation token. Pause and cancel requests are durable; a running provider must acknowledge interruption before its terminal transition. Retry creates a new Job and StageRun attempt linked to the failed/cancelled job.

At application startup, recover each persisted `running` job independently: honor cancel first, then pause, otherwise return the job and stage to `queued` with `APP_RESTART_RECOVERY`. Human review is a completed job plus a `review_required` stage and consumes no runtime slot.

## Consequences

History is append-only across retries, recovery is deterministic, and cancellation cannot fan out through a shared token. Providers must cooperate with cancellation and may need later process-tree supervision when real tools can create descendants. Milestone 1 provides only a test-target deterministic provider.
