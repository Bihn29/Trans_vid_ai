use rusqlite::{params, OptionalExtension};
use uuid::Uuid;

use super::Database;
use crate::domain::{AudioMixSettings, CoreError};

#[derive(Clone)]
pub struct AudioRepository {
    database: Database,
}

impl AudioRepository {
    pub fn new(database: Database) -> Self {
        Self { database }
    }

    pub fn get_settings(&self, project_id: Uuid) -> Result<AudioMixSettings, CoreError> {
        let connection = self.database.connection()?;
        let settings = connection
            .query_row(
                "SELECT background_gain,voice_gain,music_gain,original_voice_gain,
                        ducking_gain,fade_in_ms,fade_out_ms,target_rms_dbfs,limiter_peak
                 FROM audio_mix_configs WHERE project_id=?1",
                [project_id.to_string()],
                |row| {
                    Ok(AudioMixSettings {
                        project_id,
                        background_gain: row.get(0)?,
                        voice_gain: row.get(1)?,
                        music_gain: row.get(2)?,
                        original_voice_gain: row.get(3)?,
                        ducking_gain: row.get(4)?,
                        fade_in_ms: row.get(5)?,
                        fade_out_ms: row.get(6)?,
                        target_rms_dbfs: row.get(7)?,
                        limiter_peak: row.get(8)?,
                    })
                },
            )
            .optional()?;
        Ok(settings.unwrap_or_else(|| AudioMixSettings::defaults(project_id)))
    }

    pub fn save_settings(
        &self,
        settings: &AudioMixSettings,
    ) -> Result<AudioMixSettings, CoreError> {
        settings.validate()?;
        let connection = self.database.connection()?;
        let project_exists: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM projects WHERE id=?1)",
            [settings.project_id.to_string()],
            |row| row.get(0),
        )?;
        if !project_exists {
            return Err(CoreError::NotFound("project"));
        }
        connection.execute(
            "INSERT INTO audio_mix_configs(
                project_id,background_gain,voice_gain,music_gain,original_voice_gain,
                ducking_gain,fade_in_ms,fade_out_ms,target_rms_dbfs,limiter_peak,updated_at
             ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,strftime('%Y-%m-%dT%H:%M:%fZ','now'))
             ON CONFLICT(project_id) DO UPDATE SET
                background_gain=excluded.background_gain,voice_gain=excluded.voice_gain,
                music_gain=excluded.music_gain,original_voice_gain=excluded.original_voice_gain,
                ducking_gain=excluded.ducking_gain,fade_in_ms=excluded.fade_in_ms,
                fade_out_ms=excluded.fade_out_ms,target_rms_dbfs=excluded.target_rms_dbfs,
                limiter_peak=excluded.limiter_peak,updated_at=excluded.updated_at",
            params![
                settings.project_id.to_string(),
                settings.background_gain,
                settings.voice_gain,
                settings.music_gain,
                settings.original_voice_gain,
                settings.ducking_gain,
                settings.fade_in_ms,
                settings.fade_out_ms,
                settings.target_rms_dbfs,
                settings.limiter_peak,
            ],
        )?;
        drop(connection);
        self.get_settings(settings.project_id)
    }
}
