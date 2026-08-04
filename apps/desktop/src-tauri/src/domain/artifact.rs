use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use uuid::Uuid;

use super::CoreError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    SourceVideo,
    ProxyVideo,
    OriginalAudio,
    Vocals,
    Background,
    Music,
    Tts,
    MixedAudio,
    OverlayImage,
    Subtitle,
    Metadata,
    Preview,
    Render,
    Transcript,
    Translation,
}

impl ArtifactKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SourceVideo => "source_video",
            Self::ProxyVideo => "proxy_video",
            Self::OriginalAudio => "original_audio",
            Self::Vocals => "vocals",
            Self::Background => "background",
            Self::Music => "music",
            Self::Tts => "tts",
            Self::MixedAudio => "mixed_audio",
            Self::OverlayImage => "overlay_image",
            Self::Subtitle => "subtitle",
            Self::Metadata => "metadata",
            Self::Preview => "preview",
            Self::Render => "render",
            Self::Transcript => "transcript",
            Self::Translation => "translation",
        }
    }

    pub fn from_storage(value: &str) -> Result<Self, CoreError> {
        match value {
            "source_video" => Ok(Self::SourceVideo),
            "proxy_video" => Ok(Self::ProxyVideo),
            "original_audio" => Ok(Self::OriginalAudio),
            "vocals" => Ok(Self::Vocals),
            "background" => Ok(Self::Background),
            "music" => Ok(Self::Music),
            "tts" => Ok(Self::Tts),
            "mixed_audio" => Ok(Self::MixedAudio),
            "overlay_image" => Ok(Self::OverlayImage),
            "subtitle" => Ok(Self::Subtitle),
            "metadata" => Ok(Self::Metadata),
            "preview" => Ok(Self::Preview),
            "render" => Ok(Self::Render),
            "transcript" => Ok(Self::Transcript),
            "translation" => Ok(Self::Translation),
            _ => Err(CoreError::InvalidInput("stored artifact type")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Artifact {
    pub id: Uuid,
    pub project_id: Uuid,
    pub kind: ArtifactKind,
    pub relative_path: String,
    pub sha256: String,
    pub size_bytes: u64,
    pub created_at: String,
    pub producer_stage: String,
    pub metadata: Map<String, Value>,
}

#[derive(Debug, Clone)]
pub struct NewArtifact {
    pub id: Uuid,
    pub project_id: Uuid,
    pub kind: ArtifactKind,
    pub relative_path: String,
    pub sha256: String,
    pub size_bytes: u64,
    pub producer_stage: String,
    pub metadata: Map<String, Value>,
}

impl NewArtifact {
    pub fn validate(&self) -> Result<(), CoreError> {
        if self.relative_path.is_empty()
            || self.sha256.len() != 64
            || !self
                .sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
            || self.producer_stage.is_empty()
        {
            return Err(CoreError::InvalidInput("artifact record"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactVerification {
    Verified,
    Missing,
    Corrupt,
}
