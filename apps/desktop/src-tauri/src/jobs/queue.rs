use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};

use std::collections::HashMap;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::{
    domain::{CoreError, Job, JobStatus, NewJob, NewStageRun, StageStatus},
    persistence::{Database, JobRepository, StageRunRepository},
};

use super::{ProviderOutcome, StageProvider};

#[derive(Debug, Clone)]
pub struct ClaimedJob {
    pub job: Job,
    pub cancellation: CancellationToken,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RecoveryReport {
    pub requeued: usize,
    pub paused: usize,
    pub cancelled: usize,
}

#[derive(Clone)]
pub struct PersistentQueue {
    jobs: JobRepository,
    stages: StageRunRepository,
    in_flight: Arc<Mutex<HashMap<Uuid, CancellationToken>>>,
    max_concurrency: Arc<AtomicUsize>,
}

impl PersistentQueue {
    pub fn new(database: Database, max_concurrency: usize) -> Result<Self, CoreError> {
        if max_concurrency == 0 {
            return Err(CoreError::InvalidInput("queue concurrency"));
        }
        Ok(Self {
            jobs: JobRepository::new(database.clone()),
            stages: StageRunRepository::new(database),
            in_flight: Arc::new(Mutex::new(HashMap::new())),
            max_concurrency: Arc::new(AtomicUsize::new(max_concurrency)),
        })
    }

    pub fn set_concurrency(&self, max_concurrency: usize) -> Result<(), CoreError> {
        if max_concurrency == 0 {
            return Err(CoreError::InvalidInput("queue concurrency"));
        }
        self.max_concurrency
            .store(max_concurrency, Ordering::Release);
        Ok(())
    }

    pub fn enqueue(&self, stage: &NewStageRun, priority: i32) -> Result<Job, CoreError> {
        let stage = self.stages.insert(stage)?;
        let job = self.jobs.insert(&NewJob::first_attempt(
            stage.project_id,
            stage.stage_id,
            stage.stage_name,
            priority,
        ))?;
        self.stages
            .set_status(stage.stage_id, StageStatus::Queued, 0.0, None, None)?;
        Ok(job)
    }

    pub fn claim_next(&self) -> Result<Option<ClaimedJob>, CoreError> {
        let Some(job) = self
            .jobs
            .claim_next(self.max_concurrency.load(Ordering::Acquire))?
        else {
            return Ok(None);
        };
        let cancellation = CancellationToken::new();
        self.in_flight
            .lock()
            .map_err(|_| CoreError::LockPoisoned)?
            .insert(job.id, cancellation.clone());
        Ok(Some(ClaimedJob { job, cancellation }))
    }

    pub fn claim(&self, job_id: Uuid) -> Result<ClaimedJob, CoreError> {
        let job = self
            .jobs
            .claim(job_id, self.max_concurrency.load(Ordering::Acquire))?;
        let cancellation = CancellationToken::new();
        self.in_flight
            .lock()
            .map_err(|_| CoreError::LockPoisoned)?
            .insert(job.id, cancellation.clone());
        Ok(ClaimedJob { job, cancellation })
    }

    pub fn update_progress(&self, job_id: Uuid, progress: f64) -> Result<Job, CoreError> {
        let job = self.require_running(job_id)?;
        if progress < job.progress || !(0.0..100.0).contains(&progress) {
            return Err(CoreError::InvalidInput("job progress"));
        }
        self.stages
            .set_status(job.stage_run_id, StageStatus::Running, progress, None, None)?;
        self.jobs
            .update_status(job_id, JobStatus::Running, progress, None, None)
    }

    pub fn pause(&self, job_id: Uuid) -> Result<Job, CoreError> {
        let job = self.jobs.get(job_id)?;
        match job.status {
            JobStatus::Queued => {
                self.jobs.set_requests(job_id, true, false)?;
                self.jobs
                    .update_status(job_id, JobStatus::Paused, job.progress, None, None)
            }
            JobStatus::Running => {
                let updated = self.jobs.set_requests(job_id, true, false)?;
                self.cancel_token(job_id)?;
                Ok(updated)
            }
            JobStatus::Paused => Ok(job),
            _ => Err(CoreError::InvalidTransition),
        }
    }

    pub fn resume(&self, job_id: Uuid) -> Result<Job, CoreError> {
        let job = self.jobs.get(job_id)?;
        if job.status != JobStatus::Paused {
            return Err(CoreError::InvalidTransition);
        }
        self.jobs.set_requests(job_id, false, false)?;
        self.jobs
            .update_status(job_id, JobStatus::Queued, job.progress, None, None)
    }

    pub fn cancel(&self, job_id: Uuid) -> Result<Job, CoreError> {
        let job = self.jobs.get(job_id)?;
        match job.status {
            JobStatus::Queued | JobStatus::Paused => {
                self.jobs.set_requests(job_id, false, true)?;
                self.stages.set_status(
                    job.stage_run_id,
                    StageStatus::Cancelled,
                    job.progress,
                    None,
                    None,
                )?;
                self.jobs
                    .update_status(job_id, JobStatus::Cancelled, job.progress, None, None)
            }
            JobStatus::Running => {
                let updated = self.jobs.set_requests(job_id, false, true)?;
                self.cancel_token(job_id)?;
                Ok(updated)
            }
            JobStatus::Cancelled => Ok(job),
            _ => Err(CoreError::InvalidTransition),
        }
    }

    pub fn acknowledge_interruption(&self, job_id: Uuid) -> Result<Job, CoreError> {
        let job = self.jobs.get(job_id)?;
        if job.status != JobStatus::Running {
            return Err(CoreError::InvalidTransition);
        }
        let result = if job.cancel_requested {
            self.stages.set_status(
                job.stage_run_id,
                StageStatus::Cancelled,
                job.progress,
                None,
                None,
            )?;
            self.jobs
                .update_status(job_id, JobStatus::Cancelled, job.progress, None, None)
        } else if job.pause_requested {
            self.stages.set_status(
                job.stage_run_id,
                StageStatus::Queued,
                job.progress,
                None,
                None,
            )?;
            self.jobs
                .update_status(job_id, JobStatus::Paused, job.progress, None, None)
        } else {
            Err(CoreError::InvalidTransition)
        };
        self.release_token(job_id)?;
        result
    }

    pub fn complete(&self, job_id: Uuid, artifact_ids: &[Uuid]) -> Result<Job, CoreError> {
        let job = self.require_running(job_id)?;
        self.stages.set_outputs(job.stage_run_id, artifact_ids)?;
        self.stages
            .set_status(job.stage_run_id, StageStatus::Completed, 100.0, None, None)?;
        let completed = self
            .jobs
            .update_status(job_id, JobStatus::Completed, 100.0, None, None)?;
        self.release_token(job_id)?;
        Ok(completed)
    }

    pub fn require_review(&self, job_id: Uuid) -> Result<Job, CoreError> {
        let job = self.require_running(job_id)?;
        self.stages.set_status(
            job.stage_run_id,
            StageStatus::ReviewRequired,
            100.0,
            None,
            None,
        )?;
        let completed = self
            .jobs
            .update_status(job_id, JobStatus::Completed, 100.0, None, None)?;
        self.release_token(job_id)?;
        Ok(completed)
    }

    pub fn create_review_checkpoint(
        &self,
        stage: &NewStageRun,
        priority: i32,
    ) -> Result<Job, CoreError> {
        if !matches!(
            stage.stage_name,
            crate::domain::StageName::TranscriptReview
                | crate::domain::StageName::TranslationReview
        ) {
            return Err(CoreError::InvalidInput("review checkpoint stage"));
        }
        let job = self.enqueue(stage, priority)?;
        self.stages.set_status(
            job.stage_run_id,
            StageStatus::ReviewRequired,
            100.0,
            None,
            None,
        )?;
        self.jobs.update_status(
            job.id,
            crate::domain::JobStatus::Completed,
            100.0,
            None,
            None,
        )
    }

    pub fn complete_review(&self, project_id: Uuid) -> Result<(), CoreError> {
        self.complete_review_stage(project_id, crate::domain::StageName::TranscriptReview)
    }

    pub fn complete_translation_review(&self, project_id: Uuid) -> Result<(), CoreError> {
        self.complete_review_stage(project_id, crate::domain::StageName::TranslationReview)
    }

    fn complete_review_stage(
        &self,
        project_id: Uuid,
        stage_name: crate::domain::StageName,
    ) -> Result<(), CoreError> {
        let review = self
            .stages
            .find_latest_by_status(project_id, stage_name, StageStatus::ReviewRequired)?
            .ok_or(CoreError::NotFound("review checkpoint"))?;
        self.stages
            .set_status(review.stage_id, StageStatus::Completed, 100.0, None, None)?;
        Ok(())
    }

    pub fn fail(
        &self,
        job_id: Uuid,
        error_code: &str,
        safe_error_message: &str,
    ) -> Result<Job, CoreError> {
        let job = self.require_running(job_id)?;
        validate_error(error_code, safe_error_message)?;
        self.stages.set_status(
            job.stage_run_id,
            StageStatus::Failed,
            job.progress,
            Some(error_code),
            Some(safe_error_message),
        )?;
        let failed = self.jobs.update_status(
            job_id,
            JobStatus::Failed,
            job.progress,
            Some(error_code),
            Some(safe_error_message),
        )?;
        self.release_token(job_id)?;
        Ok(failed)
    }

    pub fn retry(&self, job_id: Uuid) -> Result<Job, CoreError> {
        let original = self.jobs.get(job_id)?;
        if !matches!(original.status, JobStatus::Failed | JobStatus::Cancelled) {
            return Err(CoreError::InvalidTransition);
        }
        let original_stage = self.stages.get(original.stage_run_id)?;
        let retry_stage = self.stages.insert_retry(&original_stage)?;
        let retry = NewJob {
            id: Uuid::new_v4(),
            project_id: original.project_id,
            stage_run_id: retry_stage.stage_id,
            job_type: original.job_type,
            priority: original.priority,
            attempt: original.attempt + 1,
            retry_of_job_id: Some(original.id),
        };
        let job = self.jobs.insert(&retry)?;
        self.stages
            .set_status(retry_stage.stage_id, StageStatus::Queued, 0.0, None, None)?;
        Ok(job)
    }

    pub fn execute<P: StageProvider>(
        &self,
        claimed: ClaimedJob,
        provider: &P,
    ) -> Result<Job, CoreError> {
        let result = provider.execute(&claimed.job, &claimed.cancellation);
        if claimed.cancellation.is_cancelled() {
            return self.acknowledge_interruption(claimed.job.id);
        }
        match result {
            Ok(ProviderOutcome::Completed { artifact_ids }) => {
                self.complete(claimed.job.id, &artifact_ids)
            }
            Ok(ProviderOutcome::ReviewRequired) => self.require_review(claimed.job.id),
            Err(_) => self.fail(
                claimed.job.id,
                "STAGE_PROVIDER_FAILED",
                "Tác vụ xử lý không thể hoàn tất.",
            ),
        }
    }

    pub fn recover_interrupted(&self) -> Result<RecoveryReport, CoreError> {
        self.in_flight
            .lock()
            .map_err(|_| CoreError::LockPoisoned)?
            .clear();
        let mut report = RecoveryReport::default();
        for job in self.jobs.list_running()? {
            if job.cancel_requested {
                self.stages.set_status(
                    job.stage_run_id,
                    StageStatus::Cancelled,
                    job.progress,
                    None,
                    None,
                )?;
                self.jobs
                    .update_status(job.id, JobStatus::Cancelled, job.progress, None, None)?;
                report.cancelled += 1;
            } else if job.pause_requested {
                self.stages.set_status(
                    job.stage_run_id,
                    StageStatus::Queued,
                    job.progress,
                    None,
                    None,
                )?;
                self.jobs
                    .update_status(job.id, JobStatus::Paused, job.progress, None, None)?;
                report.paused += 1;
            } else {
                self.stages.set_status(
                    job.stage_run_id,
                    StageStatus::Queued,
                    job.progress,
                    None,
                    None,
                )?;
                self.jobs.update_status(
                    job.id,
                    JobStatus::Queued,
                    job.progress,
                    Some("APP_RESTART_RECOVERY"),
                    Some("Tác vụ sẽ tiếp tục sau khi ứng dụng khởi động lại."),
                )?;
                report.requeued += 1;
            }
        }
        Ok(report)
    }

    pub fn in_flight_count(&self) -> Result<usize, CoreError> {
        Ok(self
            .in_flight
            .lock()
            .map_err(|_| CoreError::LockPoisoned)?
            .len())
    }

    pub fn get(&self, job_id: Uuid) -> Result<Job, CoreError> {
        self.jobs.get(job_id)
    }

    pub fn list_for_project(&self, project_id: Uuid) -> Result<Vec<Job>, CoreError> {
        self.jobs.list_for_project(project_id)
    }

    fn require_running(&self, job_id: Uuid) -> Result<Job, CoreError> {
        let job = self.jobs.get(job_id)?;
        if job.status != JobStatus::Running || job.pause_requested || job.cancel_requested {
            return Err(CoreError::InvalidTransition);
        }
        Ok(job)
    }

    fn cancel_token(&self, job_id: Uuid) -> Result<(), CoreError> {
        let guard = self.in_flight.lock().map_err(|_| CoreError::LockPoisoned)?;
        let token = guard
            .get(&job_id)
            .ok_or(CoreError::Conflict("running job token"))?;
        token.cancel();
        Ok(())
    }

    fn release_token(&self, job_id: Uuid) -> Result<(), CoreError> {
        self.in_flight
            .lock()
            .map_err(|_| CoreError::LockPoisoned)?
            .remove(&job_id);
        Ok(())
    }
}

fn validate_error(error_code: &str, safe_error_message: &str) -> Result<(), CoreError> {
    if error_code.len() < 3
        || error_code.len() > 64
        || !error_code
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
        || safe_error_message.is_empty()
        || safe_error_message.chars().count() > 500
    {
        return Err(CoreError::InvalidInput("safe job error"));
    }
    Ok(())
}
