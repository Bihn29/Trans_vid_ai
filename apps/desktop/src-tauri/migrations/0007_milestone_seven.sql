CREATE TABLE composer_configs (
    project_id TEXT PRIMARY KEY NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    config_json TEXT NOT NULL CHECK(length(config_json) BETWEEN 2 AND 65536),
    updated_at TEXT NOT NULL
) STRICT;
