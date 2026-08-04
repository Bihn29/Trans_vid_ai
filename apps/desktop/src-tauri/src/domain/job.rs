use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{CoreError, StageName};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Queued,
    Running,
    Paused,
    Completed,
    Failed,
    Cancelled,
}

impl JobStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Paused => "paused",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn from_storage(value: &str) -> Result<Self, CoreError> {
        match value {
            "queued" => Ok(Self::Queued),
            "running" => Ok(Self::Running),
            "paused" => Ok(Self::Paused),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            _ => Err(CoreError::InvalidInput("stored job status")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Job {
    pub id: Uuid,
    pub project_id: Uuid,
    pub stage_run_id: Uuid,
    pub job_type: StageName,
    pub status: JobStatus,
    pub priority: i32,
    pub progress: f64,
    pub attempt: u32,
    pub retry_of_job_id: Option<Uuid>,
    pub queued_at: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub error_code: Option<String>,
    pub safe_error_message: Option<String>,
    pub pause_requested: bool,
    pub cancel_requested: bool,
}

#[derive(Debug, Clone)]
pub struct NewJob {
    pub id: Uuid,
    pub project_id: Uuid,
    pub stage_run_id: Uuid,
    pub job_type: StageName,
    pub priority: i32,
    pub attempt: u32,
    pub retry_of_job_id: Option<Uuid>,
}

impl NewJob {
    pub fn first_attempt(
        project_id: Uuid,
        stage_run_id: Uuid,
        job_type: StageName,
        priority: i32,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            project_id,
            stage_run_id,
            job_type,
            priority,
            attempt: 1,
            retry_of_job_id: None,
        }
    }
}
