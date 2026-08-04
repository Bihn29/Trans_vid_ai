use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use uuid::Uuid;

pub const PROTOCOL_VERSION: u8 = 1;

#[derive(Debug, Clone, Serialize)]
pub struct WorkerRequest {
    pub protocol_version: u8,
    pub request_id: Uuid,
    pub action: String,
    pub project_id: Uuid,
    pub input: Map<String, Value>,
    pub config: Map<String, Value>,
    pub output_directory: String,
}

impl WorkerRequest {
    pub fn new(
        action: impl Into<String>,
        project_id: Uuid,
        output_directory: impl Into<String>,
    ) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            request_id: Uuid::new_v4(),
            action: action.into(),
            project_id,
            input: Map::new(),
            config: Map::new(),
            output_directory: output_directory.into(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct ProgressEvent {
    pub progress: u8,
    pub message: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct ArtifactOutput {
    pub r#type: String,
    pub relative_path: String,
    pub sha256: String,
    pub size_bytes: u64,
    pub metadata: Map<String, Value>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub(super) enum WorkerEvent {
    Progress {
        protocol_version: u8,
        request_id: Uuid,
        progress: u8,
        message: String,
    },
    Completed {
        protocol_version: u8,
        request_id: Uuid,
        artifacts: Vec<ArtifactOutput>,
        metrics: Map<String, Value>,
        warnings: Vec<String>,
    },
    Failed {
        protocol_version: u8,
        request_id: Uuid,
        error_code: String,
        safe_message: String,
    },
}

impl WorkerEvent {
    pub(super) fn protocol_version(&self) -> u8 {
        match self {
            Self::Progress {
                protocol_version, ..
            }
            | Self::Completed {
                protocol_version, ..
            }
            | Self::Failed {
                protocol_version, ..
            } => *protocol_version,
        }
    }

    pub(super) fn request_id(&self) -> Uuid {
        match self {
            Self::Progress { request_id, .. }
            | Self::Completed { request_id, .. }
            | Self::Failed { request_id, .. } => *request_id,
        }
    }
}
