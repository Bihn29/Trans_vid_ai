CREATE TABLE translation_blocks (
    id TEXT PRIMARY KEY NOT NULL,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    stage_run_id TEXT NOT NULL REFERENCES stage_runs(stage_id) ON DELETE CASCADE,
    block_index INTEGER NOT NULL CHECK(block_index >= 0),
    segment_ids_json TEXT NOT NULL CHECK(json_valid(segment_ids_json)),
    source_hash TEXT NOT NULL CHECK(length(source_hash) = 64),
    status TEXT NOT NULL DEFAULT 'pending'
        CHECK(status IN ('pending', 'running', 'completed', 'failed')),
    attempts INTEGER NOT NULL DEFAULT 0 CHECK(attempts >= 0),
    result_json TEXT CHECK(result_json IS NULL OR json_valid(result_json)),
    error_code TEXT,
    safe_error_message TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE(stage_run_id, block_index)
) STRICT;

CREATE INDEX idx_translation_blocks_stage
    ON translation_blocks(stage_run_id, block_index);
CREATE INDEX idx_translation_blocks_recovery
    ON translation_blocks(status, updated_at);

CREATE TABLE glossary_entries (
    id TEXT PRIMARY KEY NOT NULL,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    source_text TEXT NOT NULL CHECK(length(trim(source_text)) > 0),
    target_text TEXT NOT NULL CHECK(length(trim(target_text)) > 0),
    case_sensitive INTEGER NOT NULL DEFAULT 0 CHECK(case_sensitive IN (0, 1)),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE(project_id, source_text)
) STRICT;

CREATE TABLE locked_proper_names (
    id TEXT PRIMARY KEY NOT NULL,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    value TEXT NOT NULL CHECK(length(trim(value)) > 0),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE(project_id, value)
) STRICT;

CREATE INDEX idx_glossary_project ON glossary_entries(project_id, source_text);
CREATE INDEX idx_locked_names_project ON locked_proper_names(project_id, value);
