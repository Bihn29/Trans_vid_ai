CREATE TABLE audio_mix_configs (
    project_id TEXT PRIMARY KEY NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    background_gain REAL NOT NULL CHECK(background_gain BETWEEN 0.0 AND 2.0),
    voice_gain REAL NOT NULL CHECK(voice_gain BETWEEN 0.0 AND 2.0),
    music_gain REAL NOT NULL CHECK(music_gain BETWEEN 0.0 AND 2.0),
    original_voice_gain REAL NOT NULL CHECK(original_voice_gain BETWEEN 0.0 AND 2.0),
    ducking_gain REAL NOT NULL CHECK(ducking_gain BETWEEN 0.0 AND 1.0),
    fade_in_ms INTEGER NOT NULL CHECK(fade_in_ms BETWEEN 0 AND 2000),
    fade_out_ms INTEGER NOT NULL CHECK(fade_out_ms BETWEEN 0 AND 2000),
    target_rms_dbfs REAL NOT NULL CHECK(target_rms_dbfs BETWEEN -30.0 AND -6.0),
    limiter_peak REAL NOT NULL CHECK(limiter_peak BETWEEN 0.1 AND 1.0),
    updated_at TEXT NOT NULL
) STRICT;
