CREATE TABLE voice_assignments (
    id TEXT PRIMARY KEY NOT NULL,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    scope_type TEXT NOT NULL CHECK(scope_type IN ('project', 'speaker', 'segment')),
    scope_id TEXT,
    provider_id TEXT NOT NULL,
    voice_id TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    CHECK((scope_type = 'project' AND scope_id IS NULL) OR (scope_type != 'project' AND scope_id IS NOT NULL)),
    UNIQUE(project_id, scope_type, scope_id)
) STRICT;

CREATE TABLE tts_segment_runs (
    id TEXT PRIMARY KEY NOT NULL,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    stage_run_id TEXT NOT NULL REFERENCES stage_runs(stage_id) ON DELETE CASCADE,
    segment_id TEXT NOT NULL REFERENCES segments(id) ON DELETE CASCADE,
    cache_identity TEXT NOT NULL CHECK(length(cache_identity) = 64),
    provider_id TEXT NOT NULL,
    voice_id TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending' CHECK(status IN ('pending', 'running', 'completed', 'failed')),
    attempts INTEGER NOT NULL DEFAULT 0 CHECK(attempts >= 0),
    artifact_id TEXT REFERENCES artifacts(id),
    duration_ms INTEGER CHECK(duration_ms IS NULL OR duration_ms > 0),
    target_duration_ms INTEGER NOT NULL CHECK(target_duration_ms > 0),
    playback_rate REAL CHECK(playback_rate IS NULL OR playback_rate > 0),
    warning_code TEXT,
    error_code TEXT,
    safe_error_message TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE(stage_run_id, segment_id)
) STRICT;

CREATE INDEX idx_voice_assignments_project ON voice_assignments(project_id, scope_type, scope_id);
CREATE INDEX idx_tts_segment_runs_stage ON tts_segment_runs(stage_run_id, status);
CREATE INDEX idx_tts_segment_cache ON tts_segment_runs(project_id, segment_id, cache_identity, status);
