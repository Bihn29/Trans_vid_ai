use std::fs;

use serde_json::{Map, Value};
use uuid::Uuid;

use crate::{
    domain::{Artifact, ArtifactKind, ArtifactVerification, CoreError, NewArtifact, StageName},
    persistence::ArtifactRepository,
};

use super::{sha256_file, ProjectLayout, ProjectRelativePath};

#[derive(Clone)]
pub struct ArtifactRegistry {
    repository: ArtifactRepository,
    layout: ProjectLayout,
}

impl ArtifactRegistry {
    pub fn new(repository: ArtifactRepository, layout: ProjectLayout) -> Self {
        Self { repository, layout }
    }

    pub fn register_existing(
        &self,
        project_id: Uuid,
        kind: ArtifactKind,
        relative_path: &str,
        producer_stage: StageName,
        metadata: &Map<String, Value>,
    ) -> Result<Artifact, CoreError> {
        let relative = ProjectRelativePath::parse(relative_path)?;
        let path = self.layout.resolve_existing(project_id, &relative)?;
        let source_metadata = fs::symlink_metadata(
            self.layout
                .project_root(project_id)?
                .join(relative.as_str()),
        )?;
        if source_metadata.file_type().is_symlink() || !path.is_file() {
            return Err(CoreError::UnsafePath);
        }
        let (sha256, size_bytes) = sha256_file(&path)?;
        self.repository.insert(&NewArtifact {
            id: Uuid::new_v4(),
            project_id,
            kind,
            relative_path: relative.as_str().to_owned(),
            sha256,
            size_bytes,
            producer_stage: producer_stage.as_str().to_owned(),
            metadata: metadata.clone(),
        })
    }

    pub fn get(&self, artifact_id: Uuid) -> Result<Artifact, CoreError> {
        self.repository.get(artifact_id)
    }

    pub fn list_for_project(&self, project_id: Uuid) -> Result<Vec<Artifact>, CoreError> {
        self.repository.list_for_project(project_id)
    }

    pub fn unregister(&self, artifact_id: Uuid) -> Result<(), CoreError> {
        self.repository.delete(artifact_id)
    }

    pub fn verify(&self, artifact_id: Uuid) -> Result<ArtifactVerification, CoreError> {
        let artifact = self.repository.get(artifact_id)?;
        let relative = ProjectRelativePath::parse(&artifact.relative_path)?;
        let candidate = self.layout.prepare_output(artifact.project_id, &relative)?;
        if !candidate.exists() {
            return Ok(ArtifactVerification::Missing);
        }
        let path = match self.layout.resolve_existing(artifact.project_id, &relative) {
            Ok(path) => path,
            Err(CoreError::UnsafePath) => return Ok(ArtifactVerification::Corrupt),
            Err(error) => return Err(error),
        };
        let metadata = fs::symlink_metadata(&candidate)?;
        if metadata.file_type().is_symlink() || !path.is_file() {
            return Ok(ArtifactVerification::Corrupt);
        }
        let (sha256, size_bytes) = sha256_file(&path)?;
        if sha256 == artifact.sha256 && size_bytes == artifact.size_bytes {
            Ok(ArtifactVerification::Verified)
        } else {
            Ok(ArtifactVerification::Corrupt)
        }
    }
}
