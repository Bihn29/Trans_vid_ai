use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::domain::{CoreError, Job};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderOutcome {
    Completed { artifact_ids: Vec<Uuid> },
    ReviewRequired,
}

pub trait StageProvider: Send + Sync {
    fn execute(
        &self,
        job: &Job,
        cancellation: &CancellationToken,
    ) -> Result<ProviderOutcome, CoreError>;
}
