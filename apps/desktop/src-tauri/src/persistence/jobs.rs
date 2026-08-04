use rusqlite::{params, OptionalExtension, Row};
use uuid::Uuid;

use crate::domain::{CoreError, Job, JobStatus, NewJob, StageName};

use super::Database;

#[derive(Clone)]
pub struct JobRepository {
    database: Database,
}

impl JobRepository {
    pub fn new(database: Database) -> Self {
        Self { database }
    }

    pub fn insert(&self, job: &NewJob) -> Result<Job, CoreError> {
        if job.attempt == 0 {
            return Err(CoreError::InvalidInput("job attempt"));
        }
        let connection = self.database.connection()?;
        connection.execute(
            "INSERT INTO jobs(
                id, project_id, stage_run_id, type, status, priority, progress,
                attempt, retry_of_job_id, queued_at
             ) VALUES (
                ?1, ?2, ?3, ?4, 'queued', ?5, 0, ?6, ?7,
                strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             )",
            params![
                job.id.to_string(),
                job.project_id.to_string(),
                job.stage_run_id.to_string(),
                job.job_type.as_str(),
                job.priority,
                job.attempt,
                job.retry_of_job_id.map(|id| id.to_string()),
            ],
        )?;
        drop(connection);
        self.get(job.id)
    }

    pub fn get(&self, id: Uuid) -> Result<Job, CoreError> {
        let connection = self.database.connection()?;
        connection
            .query_row(
                &select_job_sql("WHERE id = ?1"),
                [id.to_string()],
                job_from_row,
            )
            .optional()?
            .ok_or(CoreError::NotFound("job"))
    }

    pub fn list_for_project(&self, project_id: Uuid) -> Result<Vec<Job>, CoreError> {
        let connection = self.database.connection()?;
        let mut statement = connection.prepare(&select_job_sql(
            "WHERE project_id = ?1 ORDER BY queued_at, id",
        ))?;
        let jobs = statement
            .query_map([project_id.to_string()], job_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(jobs)
    }

    pub fn list_running(&self) -> Result<Vec<Job>, CoreError> {
        let connection = self.database.connection()?;
        let mut statement = connection.prepare(&select_job_sql(
            "WHERE status = 'running' ORDER BY started_at, id",
        ))?;
        let jobs = statement
            .query_map([], job_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(jobs)
    }

    pub fn claim_next(&self, max_concurrency: usize) -> Result<Option<Job>, CoreError> {
        if max_concurrency == 0 {
            return Err(CoreError::InvalidInput("queue concurrency"));
        }
        let mut connection = self.database.connection()?;
        let transaction = connection.transaction()?;
        let running: i64 = transaction.query_row(
            "SELECT COUNT(*) FROM jobs WHERE status = 'running'",
            [],
            |row| row.get(0),
        )?;
        if running >= max_concurrency as i64 {
            transaction.commit()?;
            return Ok(None);
        }
        let next_id: Option<String> = transaction
            .query_row(
                "SELECT id FROM jobs
                 WHERE status = 'queued' AND pause_requested = 0 AND cancel_requested = 0
                 ORDER BY priority DESC, queued_at ASC, id ASC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .optional()?;
        let Some(next_id) = next_id else {
            transaction.commit()?;
            return Ok(None);
        };
        let changed = transaction.execute(
            "UPDATE jobs
             SET status = 'running', started_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                 completed_at = NULL, error_code = NULL, safe_error_message = NULL
             WHERE id = ?1 AND status = 'queued'",
            [&next_id],
        )?;
        if changed != 1 {
            return Err(CoreError::Conflict("queue claim"));
        }
        transaction.execute(
            "UPDATE stage_runs
             SET status = 'running',
                 started_at = COALESCE(started_at, strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                 completed_at = NULL,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE stage_id = (SELECT stage_run_id FROM jobs WHERE id = ?1)",
            [&next_id],
        )?;
        transaction.commit()?;
        drop(connection);
        self.get(parse_uuid(&next_id)?).map(Some)
    }

    pub fn claim(&self, id: Uuid, max_concurrency: usize) -> Result<Job, CoreError> {
        if max_concurrency == 0 {
            return Err(CoreError::InvalidInput("queue concurrency"));
        }
        let mut connection = self.database.connection()?;
        let transaction = connection.transaction()?;
        let running: i64 = transaction.query_row(
            "SELECT COUNT(*) FROM jobs WHERE status = 'running'",
            [],
            |row| row.get(0),
        )?;
        if running >= max_concurrency as i64 {
            return Err(CoreError::Conflict("queue concurrency"));
        }
        let changed = transaction.execute(
            "UPDATE jobs
             SET status = 'running', started_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                 completed_at = NULL, error_code = NULL, safe_error_message = NULL
             WHERE id = ?1 AND status = 'queued'
               AND pause_requested = 0 AND cancel_requested = 0",
            [id.to_string()],
        )?;
        if changed != 1 {
            return Err(CoreError::InvalidTransition);
        }
        transaction.execute(
            "UPDATE stage_runs
             SET status = 'running',
                 started_at = COALESCE(started_at, strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                 completed_at = NULL,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE stage_id = (SELECT stage_run_id FROM jobs WHERE id = ?1)",
            [id.to_string()],
        )?;
        transaction.commit()?;
        drop(connection);
        self.get(id)
    }

    pub fn update_status(
        &self,
        id: Uuid,
        status: JobStatus,
        progress: f64,
        error_code: Option<&str>,
        safe_error_message: Option<&str>,
    ) -> Result<Job, CoreError> {
        if !(0.0..=100.0).contains(&progress) {
            return Err(CoreError::InvalidInput("job progress"));
        }
        let finished = matches!(
            status,
            JobStatus::Completed | JobStatus::Failed | JobStatus::Cancelled
        );
        let connection = self.database.connection()?;
        let changed = connection.execute(
            "UPDATE jobs
             SET status = ?2, progress = ?3,
                 completed_at = CASE WHEN ?4 THEN strftime('%Y-%m-%dT%H:%M:%fZ', 'now') ELSE NULL END,
                 error_code = ?5, safe_error_message = ?6
             WHERE id = ?1",
            params![
                id.to_string(),
                status.as_str(),
                progress,
                finished,
                error_code,
                safe_error_message,
            ],
        )?;
        if changed != 1 {
            return Err(CoreError::NotFound("job"));
        }
        drop(connection);
        self.get(id)
    }

    pub fn set_requests(
        &self,
        id: Uuid,
        pause_requested: bool,
        cancel_requested: bool,
    ) -> Result<Job, CoreError> {
        let connection = self.database.connection()?;
        let changed = connection.execute(
            "UPDATE jobs SET pause_requested = ?2, cancel_requested = ?3 WHERE id = ?1",
            params![id.to_string(), pause_requested, cancel_requested],
        )?;
        if changed != 1 {
            return Err(CoreError::NotFound("job"));
        }
        drop(connection);
        self.get(id)
    }
}

fn select_job_sql(suffix: &str) -> String {
    format!(
        "SELECT id, project_id, stage_run_id, type, status, priority, progress,
                attempt, retry_of_job_id, queued_at, started_at, completed_at,
                error_code, safe_error_message, pause_requested, cancel_requested
         FROM jobs {suffix}"
    )
}

fn job_from_row(row: &Row<'_>) -> rusqlite::Result<Job> {
    let id: String = row.get(0)?;
    let project_id: String = row.get(1)?;
    let stage_run_id: String = row.get(2)?;
    let job_type: String = row.get(3)?;
    let status: String = row.get(4)?;
    let attempt: i64 = row.get(7)?;
    let retry_of_job_id: Option<String> = row.get(8)?;
    Ok(Job {
        id: parse_uuid(&id)?,
        project_id: parse_uuid(&project_id)?,
        stage_run_id: parse_uuid(&stage_run_id)?,
        job_type: StageName::from_storage(&job_type).map_err(|_| rusqlite::Error::InvalidQuery)?,
        status: JobStatus::from_storage(&status).map_err(|_| rusqlite::Error::InvalidQuery)?,
        priority: row.get(5)?,
        progress: row.get(6)?,
        attempt: u32::try_from(attempt).map_err(|_| rusqlite::Error::InvalidQuery)?,
        retry_of_job_id: retry_of_job_id.as_deref().map(parse_uuid).transpose()?,
        queued_at: row.get(9)?,
        started_at: row.get(10)?,
        completed_at: row.get(11)?,
        error_code: row.get(12)?,
        safe_error_message: row.get(13)?,
        pause_requested: row.get::<_, i64>(14)? != 0,
        cancel_requested: row.get::<_, i64>(15)? != 0,
    })
}

fn parse_uuid(value: &str) -> rusqlite::Result<Uuid> {
    Uuid::parse_str(value).map_err(|_| rusqlite::Error::InvalidQuery)
}
