use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use uuid::Uuid;

use super::CoreError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectStatus {
    Draft,
    Active,
    Paused,
    Completed,
    Failed,
    Cancelled,
}

impl ProjectStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Active => "active",
            Self::Paused => "paused",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn from_storage(value: &str) -> Result<Self, CoreError> {
        match value {
            "draft" => Ok(Self::Draft),
            "active" => Ok(Self::Active),
            "paused" => Ok(Self::Paused),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            _ => Err(CoreError::InvalidInput("stored project status")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowMode {
    Subtitles,
    Dubbed,
}

impl WorkflowMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Subtitles => "subtitles",
            Self::Dubbed => "dubbed",
        }
    }

    pub fn from_storage(value: &str) -> Result<Self, CoreError> {
        match value {
            "subtitles" => Ok(Self::Subtitles),
            "dubbed" => Ok(Self::Dubbed),
            _ => Err(CoreError::InvalidInput("stored workflow mode")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub id: Uuid,
    pub name: String,
    pub status: ProjectStatus,
    pub source_language: String,
    pub target_language: String,
    pub workflow_mode: WorkflowMode,
    pub source_asset_id: Option<Uuid>,
    pub config_snapshot: Map<String, Value>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct NewProject {
    pub name: String,
    pub source_language: String,
    pub target_language: String,
    pub workflow_mode: WorkflowMode,
    pub config_snapshot: Map<String, Value>,
}

impl NewProject {
    pub fn chinese_to_vietnamese(name: impl Into<String>, workflow_mode: WorkflowMode) -> Self {
        Self {
            name: name.into(),
            source_language: "zh".into(),
            target_language: "vi".into(),
            workflow_mode,
            config_snapshot: Map::new(),
        }
    }

    pub fn validate(&self) -> Result<(), CoreError> {
        validate_name(&self.name)?;
        validate_language(&self.source_language)?;
        validate_language(&self.target_language)
    }
}

#[derive(Debug, Clone, Default)]
pub struct ProjectUpdate {
    pub name: Option<String>,
    pub status: Option<ProjectStatus>,
    pub source_asset_id: Option<Option<Uuid>>,
    pub config_snapshot: Option<Map<String, Value>>,
}

impl ProjectUpdate {
    pub fn validate(&self) -> Result<(), CoreError> {
        if let Some(name) = &self.name {
            validate_name(name)?;
        }
        Ok(())
    }
}

fn validate_name(name: &str) -> Result<(), CoreError> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed.chars().count() > 120 || trimmed.chars().any(char::is_control)
    {
        return Err(CoreError::InvalidInput("project name"));
    }
    Ok(())
}

fn validate_language(language: &str) -> Result<(), CoreError> {
    if !(2..=16).contains(&language.len())
        || !language
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err(CoreError::InvalidInput("language code"));
    }
    Ok(())
}
