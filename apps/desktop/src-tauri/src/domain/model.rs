use serde::{Deserialize, Serialize};
use url::Url;

use super::CoreError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelManifest {
    pub model_id: String,
    pub provider: String,
    pub display_name: String,
    pub license: String,
    pub sends_data_off_device: bool,
    pub estimated_size_bytes: u64,
    pub schema_version: u32,
}

impl ModelManifest {
    pub fn validate(&self) -> Result<(), CoreError> {
        if self.model_id.is_empty()
            || self.model_id.len() > 128
            || self.provider.is_empty()
            || self.provider.len() > 64
            || self.display_name.is_empty()
            || self.display_name.len() > 256
            || self.license.is_empty()
            || self.license.len() > 128
            || self.schema_version == 0
        {
            return Err(CoreError::InvalidInput("model manifest"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelConsent {
    pub model_id: String,
    pub provider: String,
    pub display_name: String,
    pub license: String,
    pub sends_data_off_device: bool,
    pub estimated_size_bytes: u64,
    pub consented_at: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovedModelManifest {
    pub schema_version: u32,
    pub model_id: String,
    pub provider: String,
    pub display_name: String,
    pub version: String,
    pub license: String,
    pub source_url: String,
    pub sends_data_off_device: bool,
    pub estimated_size_bytes: u64,
    pub install_mode: String,
    pub requires_explicit_install: bool,
    pub approved_for_local_use: bool,
    pub approved_for_distribution: bool,
}

impl ApprovedModelManifest {
    pub fn validate(&self) -> Result<(), CoreError> {
        let source = Url::parse(&self.source_url)
            .map_err(|_| CoreError::InvalidInput("approved model source"))?;
        if self.schema_version != 1
            || self.model_id.is_empty()
            || self.model_id.len() > 128
            || self.provider.is_empty()
            || self.provider.len() > 64
            || self.display_name.is_empty()
            || self.display_name.len() > 256
            || self.version.is_empty()
            || self.version.len() > 128
            || self.license.is_empty()
            || self.license.len() > 128
            || source.scheme() != "https"
            || source.host_str().is_none()
            || self.estimated_size_bytes == 0
            || self.install_mode != "user_provided"
            || !self.requires_explicit_install
        {
            return Err(CoreError::InvalidInput("approved model manifest"));
        }
        Ok(())
    }

    pub fn consent_manifest(&self) -> ModelManifest {
        ModelManifest {
            model_id: self.model_id.clone(),
            provider: self.provider.clone(),
            display_name: self.display_name.clone(),
            license: self.license.clone(),
            sends_data_off_device: self.sends_data_off_device,
            estimated_size_bytes: self.estimated_size_bytes,
            schema_version: self.schema_version,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelInstallationReport {
    pub model_id: String,
    pub version: String,
    pub manifest_sha256: String,
    pub file_count: usize,
    pub total_size_bytes: u64,
}
