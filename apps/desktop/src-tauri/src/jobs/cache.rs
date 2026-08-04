use crate::{
    domain::{ArtifactVerification, CoreError, NewStageRun, StageRun, StageStatus},
    infrastructure::ArtifactRegistry,
    persistence::StageRunRepository,
};

#[derive(Clone)]
pub struct CacheResolver {
    stages: StageRunRepository,
    artifacts: ArtifactRegistry,
}

impl CacheResolver {
    pub fn new(stages: StageRunRepository, artifacts: ArtifactRegistry) -> Self {
        Self { stages, artifacts }
    }

    pub fn reusable(&self, requested: &NewStageRun) -> Result<Option<StageRun>, CoreError> {
        let cache_key = requested.cache.cache_key()?;
        let Some(candidate) = self.stages.find_completed_by_cache(
            requested.project_id,
            requested.stage_name,
            &requested.scope,
            &cache_key,
        )?
        else {
            return Ok(None);
        };

        for artifact_id in &candidate.output_artifact_ids {
            let verification = self.artifacts.verify(*artifact_id);
            if !matches!(verification, Ok(ArtifactVerification::Verified)) {
                self.stages.set_status(
                    candidate.stage_id,
                    StageStatus::Invalidated,
                    candidate.progress,
                    Some("ARTIFACT_INTEGRITY_FAILED"),
                    Some("Artifact cache verification failed."),
                )?;
                return Ok(None);
            }
        }

        Ok(Some(candidate))
    }
}
