CREATE TABLE runtime_sessions (
    session_id TEXT PRIMARY KEY NOT NULL,
    started_at TEXT NOT NULL,
    ended_at TEXT,
    clean_shutdown INTEGER NOT NULL DEFAULT 0 CHECK(clean_shutdown IN (0, 1)),
    recovered_at TEXT
) STRICT;

CREATE TABLE verified_model_installations (
    model_id TEXT PRIMARY KEY NOT NULL,
    version TEXT NOT NULL,
    manifest_sha256 TEXT NOT NULL CHECK(length(manifest_sha256) = 64),
    file_count INTEGER NOT NULL CHECK(file_count > 0),
    total_size_bytes INTEGER NOT NULL CHECK(total_size_bytes > 0),
    verified_at TEXT NOT NULL
) STRICT;

CREATE TABLE privacy_settings (
    singleton INTEGER PRIMARY KEY NOT NULL CHECK(singleton = 1),
    metadata_logging_enabled INTEGER NOT NULL CHECK(metadata_logging_enabled IN (0, 1)),
    max_log_files INTEGER NOT NULL CHECK(max_log_files BETWEEN 1 AND 10),
    max_log_file_bytes INTEGER NOT NULL CHECK(max_log_file_bytes BETWEEN 65536 AND 10485760),
    updated_at TEXT NOT NULL
) STRICT;

INSERT INTO privacy_settings(
    singleton, metadata_logging_enabled, max_log_files, max_log_file_bytes, updated_at
) VALUES (1, 1, 5, 1048576, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));
