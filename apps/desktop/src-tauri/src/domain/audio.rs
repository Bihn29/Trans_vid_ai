use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::CoreError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeparationEngineDescriptor {
    pub engine_id: String,
    pub display_name: String,
    pub version: String,
    pub license: String,
    pub install_mode: String,
    pub requires_consent: bool,
    pub sends_data_off_device: bool,
    pub approved: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioMixSettings {
    pub project_id: Uuid,
    pub background_gain: f32,
    pub voice_gain: f32,
    pub music_gain: f32,
    pub original_voice_gain: f32,
    pub ducking_gain: f32,
    pub fade_in_ms: u32,
    pub fade_out_ms: u32,
    pub target_rms_dbfs: f32,
    pub limiter_peak: f32,
}

impl AudioMixSettings {
    pub fn defaults(project_id: Uuid) -> Self {
        Self {
            project_id,
            background_gain: 0.75,
            voice_gain: 1.0,
            music_gain: 0.5,
            original_voice_gain: 0.0,
            ducking_gain: 0.4,
            fade_in_ms: 30,
            fade_out_ms: 50,
            target_rms_dbfs: -18.0,
            limiter_peak: 0.95,
        }
    }

    pub fn validate(&self) -> Result<(), CoreError> {
        let gains = [
            self.background_gain,
            self.voice_gain,
            self.music_gain,
            self.original_voice_gain,
        ];
        if gains
            .iter()
            .any(|value| !value.is_finite() || !(0.0..=2.0).contains(value))
            || !self.ducking_gain.is_finite()
            || !(0.0..=1.0).contains(&self.ducking_gain)
            || self.fade_in_ms > 2_000
            || self.fade_out_ms > 2_000
            || !self.target_rms_dbfs.is_finite()
            || !(-30.0..=-6.0).contains(&self.target_rms_dbfs)
            || !self.limiter_peak.is_finite()
            || !(0.1..=1.0).contains(&self.limiter_peak)
        {
            return Err(CoreError::InvalidInput("audio mix settings"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioQualityReport {
    pub duration_ms: u64,
    pub target_duration_ms: u64,
    pub peak_dbfs: f32,
    pub rms_dbfs: f32,
    pub clipped_samples: u64,
    pub limited_samples: u64,
    pub timeline_hash: String,
    pub separation_mode: String,
}

impl AudioQualityReport {
    pub fn passes(&self) -> bool {
        self.clipped_samples == 0
            && self.duration_ms.abs_diff(self.target_duration_ms) <= 1
            && self.timeline_hash.len() == 64
    }
}
