CREATE TABLE projects (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL CHECK(length(name) BETWEEN 1 AND 120),
    status TEXT NOT NULL CHECK(status IN ('draft', 'active', 'paused', 'completed', 'failed', 'cancelled')),
    source_language TEXT NOT NULL,
    target_language TEXT NOT NULL,
    workflow_mode TEXT NOT NULL CHECK(workflow_mode IN ('subtitles', 'dubbed')),
    source_asset_id TEXT,
    config_snapshot TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
) STRICT;

CREATE TABLE artifacts (
    id TEXT PRIMARY KEY NOT NULL,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    type TEXT NOT NULL,
    relative_path TEXT NOT NULL,
    sha256 TEXT NOT NULL CHECK(length(sha256) = 64),
    size_bytes INTEGER NOT NULL CHECK(size_bytes >= 0),
    created_at TEXT NOT NULL,
    producer_stage TEXT NOT NULL,
    metadata TEXT NOT NULL,
    UNIQUE(project_id, relative_path)
) STRICT;

CREATE TABLE stage_runs (
    stage_id TEXT PRIMARY KEY NOT NULL,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    stage_name TEXT NOT NULL,
    scope_type TEXT NOT NULL CHECK(scope_type IN ('project', 'segment', 'speaker')),
    scope_id TEXT,
    status TEXT NOT NULL CHECK(status IN (
        'pending', 'queued', 'running', 'review_required',
        'completed', 'failed', 'cancelled', 'invalidated'
    )),
    progress REAL NOT NULL CHECK(progress >= 0 AND progress <= 100),
    cache_key TEXT NOT NULL CHECK(length(cache_key) = 64),
    input_hash TEXT NOT NULL CHECK(length(input_hash) = 64),
    config_hash TEXT NOT NULL CHECK(length(config_hash) = 64),
    schema_version INTEGER NOT NULL CHECK(schema_version > 0),
    engine_name TEXT NOT NULL,
    engine_version TEXT NOT NULL,
    model_version TEXT NOT NULL,
    attempt INTEGER NOT NULL CHECK(attempt > 0),
    started_at TEXT,
    completed_at TEXT,
    error_code TEXT,
    safe_error_message TEXT,
    output_artifact_ids TEXT NOT NULL DEFAULT '[]',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    CHECK(
        (scope_type = 'project' AND scope_id IS NULL) OR
        (scope_type IN ('segment', 'speaker') AND scope_id IS NOT NULL)
    )
) STRICT;

CREATE TABLE jobs (
    id TEXT PRIMARY KEY NOT NULL,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    stage_run_id TEXT NOT NULL REFERENCES stage_runs(stage_id) ON DELETE CASCADE,
    type TEXT NOT NULL,
    status TEXT NOT NULL CHECK(status IN (
        'queued', 'running', 'paused', 'completed', 'failed', 'cancelled'
    )),
    priority INTEGER NOT NULL,
    progress REAL NOT NULL CHECK(progress >= 0 AND progress <= 100),
    attempt INTEGER NOT NULL CHECK(attempt > 0),
    retry_of_job_id TEXT REFERENCES jobs(id),
    queued_at TEXT NOT NULL,
    started_at TEXT,
    completed_at TEXT,
    error_code TEXT,
    safe_error_message TEXT,
    pause_requested INTEGER NOT NULL DEFAULT 0 CHECK(pause_requested IN (0, 1)),
    cancel_requested INTEGER NOT NULL DEFAULT 0 CHECK(cancel_requested IN (0, 1))
) STRICT;

CREATE INDEX idx_artifacts_project ON artifacts(project_id);
CREATE INDEX idx_stage_runs_project_stage ON stage_runs(project_id, stage_name, status);
CREATE INDEX idx_stage_runs_scope ON stage_runs(project_id, scope_type, scope_id);
CREATE INDEX idx_jobs_claim ON jobs(status, priority DESC, queued_at, id);
CREATE INDEX idx_jobs_project ON jobs(project_id, status);

