use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::CoreError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoiceDescriptor {
    pub provider_id: String,
    pub voice_id: String,
    pub display_name: String,
    pub language: String,
    pub sends_data_off_device: bool,
    pub approved: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type", content = "id")]
pub enum VoiceScope {
    Project,
    Speaker(Uuid),
    Segment(Uuid),
}

impl VoiceScope {
    pub fn to_storage(&self) -> (&'static str, Option<String>) {
        match self {
            Self::Project => ("project", None),
            Self::Speaker(id) => ("speaker", Some(id.to_string())),
            Self::Segment(id) => ("segment", Some(id.to_string())),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoiceAssignment {
    pub id: Uuid,
    pub project_id: Uuid,
    pub scope: VoiceScope,
    pub provider_id: String,
    pub voice_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TtsRunStatus {
    Pending,
    Running,
    Completed,
    Failed,
}

impl TtsRunStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }
    pub fn from_storage(value: &str) -> Result<Self, CoreError> {
        match value {
            "pending" => Ok(Self::Pending),
            "running" => Ok(Self::Running),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            _ => Err(CoreError::InvalidInput("stored TTS status")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TtsSegmentRun {
    pub id: Uuid,
    pub project_id: Uuid,
    pub stage_run_id: Uuid,
    pub segment_id: Uuid,
    pub cache_identity: String,
    pub provider_id: String,
    pub voice_id: String,
    pub status: TtsRunStatus,
    pub attempts: u32,
    pub artifact_id: Option<Uuid>,
    pub duration_ms: Option<u64>,
    pub target_duration_ms: u64,
    pub playback_rate: Option<f64>,
    pub warning_code: Option<String>,
    pub error_code: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NewTtsSegmentRun {
    pub id: Uuid,
    pub project_id: Uuid,
    pub stage_run_id: Uuid,
    pub segment_id: Uuid,
    pub cache_identity: String,
    pub provider_id: String,
    pub voice_id: String,
    pub target_duration_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DurationFit {
    pub playback_rate: f64,
    pub warning_code: Option<&'static str>,
}

pub fn duration_fit(duration_ms: u64, target_ms: u64) -> Result<DurationFit, CoreError> {
    if duration_ms == 0 || target_ms == 0 {
        return Err(CoreError::InvalidInput("TTS duration"));
    }
    let rate = duration_ms as f64 / target_ms as f64;
    let warning = if rate > 1.2 {
        Some("SHORTEN_TRANSLATION")
    } else if rate < 0.85 {
        Some("EXCESSIVE_SLOWDOWN")
    } else {
        None
    };
    Ok(DurationFit {
        playback_rate: rate.clamp(0.85, 1.2),
        warning_code: warning,
    })
}
