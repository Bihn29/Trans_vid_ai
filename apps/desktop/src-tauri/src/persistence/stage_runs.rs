use rusqlite::{params, OptionalExtension, Row};
use uuid::Uuid;

use crate::domain::{CoreError, NewStageRun, StageName, StageRun, StageScope, StageStatus};

use super::Database;

#[derive(Clone)]
pub struct StageRunRepository {
    database: Database,
}

impl StageRunRepository {
    pub fn new(database: Database) -> Self {
        Self { database }
    }

    pub fn insert(&self, run: &NewStageRun) -> Result<StageRun, CoreError> {
        run.cache.validate()?;
        if run.attempt == 0
            || run.engine_name != run.cache.engine_name
            || run.engine_version != run.cache.engine_version
            || run.model_version != run.cache.model_version
        {
            return Err(CoreError::InvalidInput("stage run"));
        }
        let cache_key = run.cache.cache_key()?;
        let (scope_type, scope_id) = run.scope.to_storage();
        let connection = self.database.connection()?;
        connection.execute(
            "INSERT INTO stage_runs(
                stage_id, project_id, stage_name, scope_type, scope_id, status, progress,
                cache_key, input_hash, config_hash, schema_version, engine_name,
                engine_version, model_version, attempt, output_artifact_ids,
                created_at, updated_at
             ) VALUES (
                ?1, ?2, ?3, ?4, ?5, 'pending', 0,
                ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, '[]',
                strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             )",
            params![
                run.stage_id.to_string(),
                run.project_id.to_string(),
                run.stage_name.as_str(),
                scope_type,
                scope_id,
                cache_key,
                run.cache.input_hash,
                run.cache.config_hash,
                run.cache.schema_version,
                run.engine_name,
                run.engine_version,
                run.model_version,
                run.attempt,
            ],
        )?;
        drop(connection);
        self.get(run.stage_id)
    }

    pub fn get(&self, id: Uuid) -> Result<StageRun, CoreError> {
        let connection = self.database.connection()?;
        connection
            .query_row(
                "SELECT stage_id, project_id, stage_name, scope_type, scope_id, status,
                        progress, cache_key, input_hash, config_hash, schema_version,
                        engine_name, engine_version, model_version, attempt, started_at,
                        completed_at, error_code, safe_error_message, output_artifact_ids,
                        created_at, updated_at
                 FROM stage_runs WHERE stage_id = ?1",
                [id.to_string()],
                stage_run_from_row,
            )
            .optional()?
            .ok_or(CoreError::NotFound("stage run"))
    }

    pub fn list_for_project(&self, project_id: Uuid) -> Result<Vec<StageRun>, CoreError> {
        let connection = self.database.connection()?;
        let mut statement = connection.prepare(
            "SELECT stage_id, project_id, stage_name, scope_type, scope_id, status,
                    progress, cache_key, input_hash, config_hash, schema_version,
                    engine_name, engine_version, model_version, attempt, started_at,
                    completed_at, error_code, safe_error_message, output_artifact_ids,
                    created_at, updated_at
             FROM stage_runs WHERE project_id = ?1 ORDER BY created_at, stage_id",
        )?;
        let runs = statement
            .query_map([project_id.to_string()], stage_run_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(runs)
    }

    pub fn find_completed_by_cache(
        &self,
        project_id: Uuid,
        stage_name: StageName,
        scope: &StageScope,
        cache_key: &str,
    ) -> Result<Option<StageRun>, CoreError> {
        if cache_key.len() != 64
            || !cache_key
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return Err(CoreError::InvalidInput("cache key"));
        }
        let (scope_type, scope_id) = scope.to_storage();
        let connection = self.database.connection()?;
        connection
            .query_row(
                "SELECT stage_id, project_id, stage_name, scope_type, scope_id, status,
                        progress, cache_key, input_hash, config_hash, schema_version,
                        engine_name, engine_version, model_version, attempt, started_at,
                        completed_at, error_code, safe_error_message, output_artifact_ids,
                        created_at, updated_at
                 FROM stage_runs
                 WHERE project_id = ?1 AND stage_name = ?2
                   AND scope_type = ?3 AND scope_id IS ?4
                   AND cache_key = ?5 AND status = 'completed'
                 ORDER BY completed_at DESC, stage_id DESC
                 LIMIT 1",
                params![
                    project_id.to_string(),
                    stage_name.as_str(),
                    scope_type,
                    scope_id,
                    cache_key,
                ],
                stage_run_from_row,
            )
            .optional()
            .map_err(CoreError::from)
    }

    pub fn find_latest_by_status(
        &self,
        project_id: Uuid,
        stage_name: StageName,
        status: StageStatus,
    ) -> Result<Option<StageRun>, CoreError> {
        let connection = self.database.connection()?;
        connection
            .query_row(
                "SELECT stage_id, project_id, stage_name, scope_type, scope_id, status,
                        progress, cache_key, input_hash, config_hash, schema_version,
                        engine_name, engine_version, model_version, attempt, started_at,
                        completed_at, error_code, safe_error_message, output_artifact_ids,
                        created_at, updated_at
                 FROM stage_runs
                 WHERE project_id = ?1 AND stage_name = ?2 AND status = ?3
                 ORDER BY updated_at DESC, stage_id DESC
                 LIMIT 1",
                params![project_id.to_string(), stage_name.as_str(), status.as_str()],
                stage_run_from_row,
            )
            .optional()
            .map_err(CoreError::from)
    }

    pub fn set_status(
        &self,
        id: Uuid,
        status: StageStatus,
        progress: f64,
        error_code: Option<&str>,
        safe_error_message: Option<&str>,
    ) -> Result<StageRun, CoreError> {
        if !(0.0..=100.0).contains(&progress) {
            return Err(CoreError::InvalidInput("stage progress"));
        }
        let started = matches!(status, StageStatus::Running);
        let finished = matches!(
            status,
            StageStatus::ReviewRequired
                | StageStatus::Completed
                | StageStatus::Failed
                | StageStatus::Cancelled
                | StageStatus::Invalidated
        );
        let connection = self.database.connection()?;
        let changed = connection.execute(
            "UPDATE stage_runs
             SET status = ?2, progress = ?3,
                 started_at = CASE WHEN ?4 THEN COALESCE(started_at, strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) ELSE started_at END,
                 completed_at = CASE WHEN ?5 THEN strftime('%Y-%m-%dT%H:%M:%fZ', 'now') ELSE NULL END,
                 error_code = ?6, safe_error_message = ?7,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE stage_id = ?1",
            params![
                id.to_string(),
                status.as_str(),
                progress,
                started,
                finished,
                error_code,
                safe_error_message,
            ],
        )?;
        if changed != 1 {
            return Err(CoreError::NotFound("stage run"));
        }
        drop(connection);
        self.get(id)
    }

    pub fn set_outputs(&self, id: Uuid, artifact_ids: &[Uuid]) -> Result<(), CoreError> {
        let output = serde_json::to_string(artifact_ids)
            .map_err(|_| CoreError::InvalidInput("stage artifacts"))?;
        let connection = self.database.connection()?;
        let changed = connection.execute(
            "UPDATE stage_runs
             SET output_artifact_ids = ?2,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE stage_id = ?1",
            params![id.to_string(), output],
        )?;
        if changed != 1 {
            return Err(CoreError::NotFound("stage run"));
        }
        Ok(())
    }

    pub fn insert_retry(&self, original: &StageRun) -> Result<StageRun, CoreError> {
        let new_id = Uuid::new_v4();
        let connection = self.database.connection()?;
        connection.execute(
            "INSERT INTO stage_runs(
                stage_id, project_id, stage_name, scope_type, scope_id, status, progress,
                cache_key, input_hash, config_hash, schema_version, engine_name,
                engine_version, model_version, attempt, output_artifact_ids,
                created_at, updated_at
             )
             SELECT ?1, project_id, stage_name, scope_type, scope_id, 'pending', 0,
                    cache_key, input_hash, config_hash, schema_version, engine_name,
                    engine_version, model_version, attempt + 1, '[]',
                    strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                    strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             FROM stage_runs WHERE stage_id = ?2",
            params![new_id.to_string(), original.stage_id.to_string()],
        )?;
        drop(connection);
        self.get(new_id)
    }
}

fn stage_run_from_row(row: &Row<'_>) -> rusqlite::Result<StageRun> {
    let stage_id: String = row.get(0)?;
    let project_id: String = row.get(1)?;
    let stage_name: String = row.get(2)?;
    let scope_type: String = row.get(3)?;
    let scope_id: Option<String> = row.get(4)?;
    let status: String = row.get(5)?;
    let schema_version: i64 = row.get(10)?;
    let attempt: i64 = row.get(14)?;
    let artifact_ids: String = row.get(19)?;
    Ok(StageRun {
        stage_id: parse_uuid(&stage_id)?,
        project_id: parse_uuid(&project_id)?,
        stage_name: StageName::from_storage(&stage_name)
            .map_err(|_| rusqlite::Error::InvalidQuery)?,
        scope: StageScope::from_storage(&scope_type, scope_id.as_deref())
            .map_err(|_| rusqlite::Error::InvalidQuery)?,
        status: StageStatus::from_storage(&status).map_err(|_| rusqlite::Error::InvalidQuery)?,
        progress: row.get(6)?,
        cache_key: row.get(7)?,
        input_hash: row.get(8)?,
        config_hash: row.get(9)?,
        schema_version: u32::try_from(schema_version).map_err(|_| rusqlite::Error::InvalidQuery)?,
        engine_name: row.get(11)?,
        engine_version: row.get(12)?,
        model_version: row.get(13)?,
        attempt: u32::try_from(attempt).map_err(|_| rusqlite::Error::InvalidQuery)?,
        started_at: row.get(15)?,
        completed_at: row.get(16)?,
        error_code: row.get(17)?,
        safe_error_message: row.get(18)?,
        output_artifact_ids: serde_json::from_str(&artifact_ids)
            .map_err(|_| rusqlite::Error::InvalidQuery)?,
        created_at: row.get(20)?,
        updated_at: row.get(21)?,
    })
}

fn parse_uuid(value: &str) -> rusqlite::Result<Uuid> {
    Uuid::parse_str(value).map_err(|_| rusqlite::Error::InvalidQuery)
}
