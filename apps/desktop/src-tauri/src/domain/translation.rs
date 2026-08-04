use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{CoreError, Segment};

pub const MAX_TRANSLATION_BLOCK_SEGMENTS: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TranslationBlockStatus {
    Pending,
    Running,
    Completed,
    Failed,
}

impl TranslationBlockStatus {
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
            _ => Err(CoreError::InvalidInput("stored translation block status")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranslationBlock {
    pub id: Uuid,
    pub project_id: Uuid,
    pub stage_run_id: Uuid,
    pub block_index: u32,
    pub segment_ids: Vec<Uuid>,
    pub source_hash: String,
    pub status: TranslationBlockStatus,
    pub attempts: u32,
    pub result: Option<TranslationResult>,
    pub error_code: Option<String>,
    pub safe_error_message: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct NewTranslationBlock {
    pub id: Uuid,
    pub project_id: Uuid,
    pub stage_run_id: Uuid,
    pub block_index: u32,
    pub segment_ids: Vec<Uuid>,
    pub source_hash: String,
}

impl NewTranslationBlock {
    pub fn validate(&self) -> Result<(), CoreError> {
        if self.segment_ids.is_empty()
            || self.segment_ids.len() > MAX_TRANSLATION_BLOCK_SEGMENTS
            || self.source_hash.len() != 64
            || !self
                .source_hash
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return Err(CoreError::InvalidInput("translation block"));
        }
        let unique = self
            .segment_ids
            .iter()
            .collect::<std::collections::HashSet<_>>();
        if unique.len() != self.segment_ids.len() {
            return Err(CoreError::InvalidInput("translation block segment IDs"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranslationItem {
    pub id: Uuid,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranslationResult {
    pub schema_version: u32,
    pub translations: Vec<TranslationItem>,
}

impl TranslationResult {
    pub fn validate_exact(&self, expected_ids: &[Uuid]) -> Result<(), CoreError> {
        if self.schema_version != 1 || self.translations.len() != expected_ids.len() {
            return Err(CoreError::InvalidTranslationOutput);
        }
        let expected = expected_ids
            .iter()
            .copied()
            .collect::<std::collections::HashSet<_>>();
        let mut seen = std::collections::HashSet::new();
        for item in &self.translations {
            if item.text.trim().is_empty() || !expected.contains(&item.id) || !seen.insert(item.id)
            {
                return Err(CoreError::InvalidTranslationOutput);
            }
        }
        if seen != expected {
            return Err(CoreError::InvalidTranslationOutput);
        }
        Ok(())
    }

    pub fn validate_locked_names(
        &self,
        sources: &std::collections::HashMap<Uuid, String>,
        locked_names: &[String],
    ) -> Result<(), CoreError> {
        for item in &self.translations {
            let source = sources
                .get(&item.id)
                .ok_or(CoreError::InvalidTranslationOutput)?;
            if locked_names
                .iter()
                .any(|name| source.contains(name) && !item.text.contains(name))
            {
                return Err(CoreError::InvalidTranslationOutput);
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GlossaryEntry {
    pub id: Uuid,
    pub project_id: Uuid,
    pub source_text: String,
    pub target_text: String,
    pub case_sensitive: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockedProperName {
    pub id: Uuid,
    pub project_id: Uuid,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TranslationProviderDisclosure {
    pub provider_id: String,
    pub display_name: String,
    pub sends_data_off_device: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct TranslationSegmentInput {
    pub id: Uuid,
    pub source_text: String,
}

impl From<&Segment> for TranslationSegmentInput {
    fn from(segment: &Segment) -> Self {
        Self {
            id: segment.id,
            source_text: segment.source_text.clone(),
        }
    }
}
