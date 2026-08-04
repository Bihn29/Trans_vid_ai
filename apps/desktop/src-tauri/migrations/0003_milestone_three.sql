CREATE TABLE model_consents (
    model_id TEXT PRIMARY KEY NOT NULL,
    provider TEXT NOT NULL,
    display_name TEXT NOT NULL,
    license TEXT NOT NULL,
    sends_data_off_device INTEGER NOT NULL CHECK(sends_data_off_device IN (0, 1)),
    estimated_size_bytes INTEGER NOT NULL CHECK(estimated_size_bytes >= 0),
    consented_at TEXT NOT NULL,
    created_at TEXT NOT NULL
) STRICT;

CREATE TABLE segments (
    id TEXT PRIMARY KEY NOT NULL,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    sequence INTEGER NOT NULL CHECK(sequence >= 0),
    start_ms INTEGER NOT NULL CHECK(start_ms >= 0),
    end_ms INTEGER NOT NULL CHECK(end_ms >= 1),
    source_text TEXT NOT NULL,
    translated_text TEXT NOT NULL DEFAULT '',
    speaker_id TEXT,
    voice_id TEXT,
    asr_confidence REAL CHECK(asr_confidence >= 0 AND asr_confidence <= 1),
    estimated_duration_ms INTEGER CHECK(estimated_duration_ms >= 0),
    target_duration_ms INTEGER CHECK(target_duration_ms >= 0),
    playback_rate REAL CHECK(playback_rate > 0) DEFAULT 1.0,
    enabled INTEGER NOT NULL DEFAULT 1 CHECK(enabled IN (0, 1)),
    review_status TEXT NOT NULL DEFAULT 'unreviewed'
        CHECK(review_status IN ('unreviewed', 'approved', 'needs_attention')),
    source_hash TEXT CHECK(source_hash IS NULL OR length(source_hash) = 64),
    translation_hash TEXT CHECK(translation_hash IS NULL OR length(translation_hash) = 64),
    voice_hash TEXT CHECK(voice_hash IS NULL OR length(voice_hash) = 64),
    audio_artifact_id TEXT REFERENCES artifacts(id),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    CHECK(end_ms > start_ms),
    UNIQUE(project_id, sequence)
) STRICT;

CREATE INDEX idx_segments_project ON segments(project_id, sequence);
CREATE INDEX idx_segments_speaker ON segments(project_id, speaker_id);
