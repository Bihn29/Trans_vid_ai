use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::CoreError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum StageName {
    Import,
    Probe,
    Normalize,
    ExtractAudio,
    SeparateAudio,
    Transcribe,
    Segment,
    TranscriptReview,
    Translate,
    TranslationReview,
    VoiceAssignment,
    VoicePreview,
    Synthesize,
    FitDuration,
    MixAudio,
    ComposeVideo,
    QualityCheck,
    Render,
    Complete,
}

impl StageName {
    pub const ALL: [Self; 19] = [
        Self::Import,
        Self::Probe,
        Self::Normalize,
        Self::ExtractAudio,
        Self::SeparateAudio,
        Self::Transcribe,
        Self::Segment,
        Self::TranscriptReview,
        Self::Translate,
        Self::TranslationReview,
        Self::VoiceAssignment,
        Self::VoicePreview,
        Self::Synthesize,
        Self::FitDuration,
        Self::MixAudio,
        Self::ComposeVideo,
        Self::QualityCheck,
        Self::Render,
        Self::Complete,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Import => "IMPORT",
            Self::Probe => "PROBE",
            Self::Normalize => "NORMALIZE",
            Self::ExtractAudio => "EXTRACT_AUDIO",
            Self::SeparateAudio => "SEPARATE_AUDIO",
            Self::Transcribe => "TRANSCRIBE",
            Self::Segment => "SEGMENT",
            Self::TranscriptReview => "TRANSCRIPT_REVIEW",
            Self::Translate => "TRANSLATE",
            Self::TranslationReview => "TRANSLATION_REVIEW",
            Self::VoiceAssignment => "VOICE_ASSIGNMENT",
            Self::VoicePreview => "VOICE_PREVIEW",
            Self::Synthesize => "SYNTHESIZE",
            Self::FitDuration => "FIT_DURATION",
            Self::MixAudio => "MIX_AUDIO",
            Self::ComposeVideo => "COMPOSE_VIDEO",
            Self::QualityCheck => "QUALITY_CHECK",
            Self::Render => "RENDER",
            Self::Complete => "COMPLETE",
        }
    }

    pub fn from_storage(value: &str) -> Result<Self, CoreError> {
        Self::ALL
            .into_iter()
            .find(|stage| stage.as_str() == value)
            .ok_or(CoreError::InvalidInput("stored stage name"))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StageStatus {
    Pending,
    Queued,
    Running,
    ReviewRequired,
    Completed,
    Failed,
    Cancelled,
    Invalidated,
}

impl StageStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Queued => "queued",
            Self::Running => "running",
            Self::ReviewRequired => "review_required",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Invalidated => "invalidated",
        }
    }

    pub fn from_storage(value: &str) -> Result<Self, CoreError> {
        match value {
            "pending" => Ok(Self::Pending),
            "queued" => Ok(Self::Queued),
            "running" => Ok(Self::Running),
            "review_required" => Ok(Self::ReviewRequired),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            "invalidated" => Ok(Self::Invalidated),
            _ => Err(CoreError::InvalidInput("stored stage status")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "id", rename_all = "snake_case")]
pub enum StageScope {
    Project,
    Segment(Uuid),
    Speaker(Uuid),
}

impl StageScope {
    pub fn to_storage(&self) -> (&'static str, Option<String>) {
        match self {
            Self::Project => ("project", None),
            Self::Segment(id) => ("segment", Some(id.to_string())),
            Self::Speaker(id) => ("speaker", Some(id.to_string())),
        }
    }

    pub fn from_storage(kind: &str, id: Option<&str>) -> Result<Self, CoreError> {
        match (kind, id) {
            ("project", None) => Ok(Self::Project),
            ("segment", Some(id)) => Uuid::parse_str(id)
                .map(Self::Segment)
                .map_err(|_| CoreError::InvalidInput("stored segment scope")),
            ("speaker", Some(id)) => Uuid::parse_str(id)
                .map(Self::Speaker)
                .map_err(|_| CoreError::InvalidInput("stored speaker scope")),
            _ => Err(CoreError::InvalidInput("stored stage scope")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StageRun {
    pub stage_id: Uuid,
    pub project_id: Uuid,
    pub stage_name: StageName,
    pub scope: StageScope,
    pub status: StageStatus,
    pub progress: f64,
    pub cache_key: String,
    pub input_hash: String,
    pub config_hash: String,
    pub schema_version: u32,
    pub engine_name: String,
    pub engine_version: String,
    pub model_version: String,
    pub attempt: u32,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub error_code: Option<String>,
    pub safe_error_message: Option<String>,
    pub output_artifact_ids: Vec<Uuid>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct NewStageRun {
    pub stage_id: Uuid,
    pub project_id: Uuid,
    pub stage_name: StageName,
    pub scope: StageScope,
    pub cache: CacheDescriptor,
    pub engine_name: String,
    pub engine_version: String,
    pub model_version: String,
    pub attempt: u32,
}

impl NewStageRun {
    pub fn new(
        project_id: Uuid,
        stage_name: StageName,
        scope: StageScope,
        cache: CacheDescriptor,
        engine_name: impl Into<String>,
    ) -> Self {
        Self {
            stage_id: Uuid::new_v4(),
            project_id,
            stage_name,
            scope,
            cache,
            engine_name: engine_name.into(),
            engine_version: "1".into(),
            model_version: "none".into(),
            attempt: 1,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CacheDescriptor {
    pub schema_version: u32,
    pub input_hash: String,
    pub config_hash: String,
    pub engine_name: String,
    pub engine_version: String,
    pub model_version: String,
    pub metadata: Map<String, Value>,
}

impl CacheDescriptor {
    pub fn validate(&self) -> Result<(), CoreError> {
        if self.schema_version == 0
            || !is_sha256(&self.input_hash)
            || !is_sha256(&self.config_hash)
            || self.engine_name.is_empty()
            || self.engine_version.is_empty()
            || self.model_version.is_empty()
        {
            return Err(CoreError::InvalidInput("cache descriptor"));
        }
        Ok(())
    }

    pub fn cache_key(&self) -> Result<String, CoreError> {
        self.validate()?;
        let metadata = canonical_json_object(&self.metadata);
        let mut hasher = Sha256::new();
        hasher.update(b"VIETDUB_STAGE_CACHE\0");
        hasher.update(self.schema_version.to_be_bytes());
        for value in [
            self.input_hash.as_bytes(),
            self.config_hash.as_bytes(),
            self.engine_name.as_bytes(),
            self.engine_version.as_bytes(),
            self.model_version.as_bytes(),
            metadata.as_bytes(),
        ] {
            hasher.update((value.len() as u64).to_be_bytes());
            hasher.update(value);
        }
        Ok(format!("{:x}", hasher.finalize()))
    }
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn canonical_json_object(value: &Map<String, Value>) -> String {
    let ordered = value
        .iter()
        .map(|(key, value)| (key.clone(), canonical_json(value)))
        .collect::<std::collections::BTreeMap<_, _>>();
    serde_json::to_string(&ordered).expect("JSON values are serializable")
}

fn canonical_json(value: &Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, value)| (key.clone(), canonical_json(value)))
                .collect::<std::collections::BTreeMap<_, _>>()
                .into_iter()
                .collect(),
        ),
        Value::Array(values) => Value::Array(values.iter().map(canonical_json).collect()),
        _ => value.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn cache_key_is_stable_across_metadata_key_order() {
        let mut first = Map::new();
        first.insert("b".into(), json!({"z": 2, "a": 1}));
        first.insert("a".into(), json!(true));
        let mut second = Map::new();
        second.insert("a".into(), json!(true));
        second.insert("b".into(), json!({"a": 1, "z": 2}));

        let descriptor = |metadata| CacheDescriptor {
            schema_version: 1,
            input_hash: "a".repeat(64),
            config_hash: "b".repeat(64),
            engine_name: "deterministic".into(),
            engine_version: "1".into(),
            model_version: "none".into(),
            metadata,
        };

        let first_key = descriptor(first).cache_key().expect("first key");
        let second_key = descriptor(second).cache_key().expect("second key");
        assert_eq!(first_key, second_key);
        assert_eq!(
            first_key,
            "bb24a5bce8fde7530b0f447b7b550260283b41078c5c7698294cd0867ca2cf42"
        );
    }
}
