use rusqlite::{params, OptionalExtension, Row};
use uuid::Uuid;

use crate::domain::{
    compute_text_hash, CoreError, GlossaryEntry, LockedProperName, NewTranslationBlock,
    TranslationBlock, TranslationBlockStatus, TranslationResult,
};

use super::Database;

#[derive(Clone)]
pub struct TranslationRepository {
    database: Database,
}

impl TranslationRepository {
    pub fn new(database: Database) -> Self {
        Self { database }
    }

    pub fn insert_blocks(
        &self,
        blocks: &[NewTranslationBlock],
    ) -> Result<Vec<TranslationBlock>, CoreError> {
        for block in blocks {
            block.validate()?;
        }
        let mut connection = self.database.connection()?;
        let transaction = connection.transaction()?;
        for block in blocks {
            let segment_ids = serde_json::to_string(&block.segment_ids)
                .map_err(|_| CoreError::InvalidInput("translation block segment IDs"))?;
            let reusable: Option<String> = transaction
                .query_row(
                    "SELECT result_json FROM translation_blocks
                     WHERE project_id = ?1 AND segment_ids_json = ?2 AND source_hash = ?3
                       AND status = 'completed' AND result_json IS NOT NULL
                     ORDER BY updated_at DESC LIMIT 1",
                    params![block.project_id.to_string(), segment_ids, block.source_hash],
                    |row| row.get(0),
                )
                .optional()?;
            let status = if reusable.is_some() {
                TranslationBlockStatus::Completed.as_str()
            } else {
                TranslationBlockStatus::Pending.as_str()
            };
            transaction.execute(
                "INSERT INTO translation_blocks(
                    id, project_id, stage_run_id, block_index, segment_ids_json,
                    source_hash, status, attempts, result_json, error_code,
                    safe_error_message, created_at, updated_at
                 ) VALUES (
                    ?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, ?8, NULL, NULL,
                    strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                    strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                 )",
                params![
                    block.id.to_string(),
                    block.project_id.to_string(),
                    block.stage_run_id.to_string(),
                    block.block_index,
                    segment_ids,
                    block.source_hash,
                    status,
                    reusable,
                ],
            )?;
        }
        transaction.commit()?;
        drop(connection);
        blocks.iter().map(|block| self.get(block.id)).collect()
    }

    pub fn get(&self, id: Uuid) -> Result<TranslationBlock, CoreError> {
        let connection = self.database.connection()?;
        connection
            .query_row(
                &select_blocks("WHERE id = ?1"),
                [id.to_string()],
                block_from_row,
            )
            .optional()?
            .ok_or(CoreError::NotFound("translation block"))
    }

    pub fn list_for_stage(&self, stage_run_id: Uuid) -> Result<Vec<TranslationBlock>, CoreError> {
        let connection = self.database.connection()?;
        let mut statement = connection.prepare(&select_blocks(
            "WHERE stage_run_id = ?1 ORDER BY block_index ASC",
        ))?;
        let values = statement
            .query_map([stage_run_id.to_string()], block_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(values)
    }

    pub fn recover_stage(&self, stage_run_id: Uuid) -> Result<usize, CoreError> {
        let connection = self.database.connection()?;
        Ok(connection.execute(
            "UPDATE translation_blocks
             SET status = 'pending', error_code = 'APP_RESTART_RECOVERY',
                 safe_error_message = 'Khối dịch sẽ tiếp tục sau khi ứng dụng khởi động lại.',
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE stage_run_id = ?1 AND status = 'running'",
            [stage_run_id.to_string()],
        )?)
    }

    pub fn mark_running(&self, id: Uuid) -> Result<TranslationBlock, CoreError> {
        let connection = self.database.connection()?;
        let changed = connection.execute(
            "UPDATE translation_blocks
             SET status = 'running', attempts = attempts + 1,
                 error_code = NULL, safe_error_message = NULL,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ?1 AND status IN ('pending', 'failed')",
            [id.to_string()],
        )?;
        if changed != 1 {
            return Err(CoreError::InvalidTransition);
        }
        drop(connection);
        self.get(id)
    }

    pub fn reset_pending(&self, id: Uuid) -> Result<(), CoreError> {
        let connection = self.database.connection()?;
        let changed = connection.execute(
            "UPDATE translation_blocks
             SET status = 'pending', updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ?1 AND status = 'running'",
            [id.to_string()],
        )?;
        if changed != 1 {
            return Err(CoreError::InvalidTransition);
        }
        Ok(())
    }

    pub fn fail(
        &self,
        id: Uuid,
        error_code: &str,
        safe_error_message: &str,
    ) -> Result<TranslationBlock, CoreError> {
        if error_code.is_empty() || error_code.len() > 64 || safe_error_message.is_empty() {
            return Err(CoreError::InvalidInput("translation block error"));
        }
        let connection = self.database.connection()?;
        let changed = connection.execute(
            "UPDATE translation_blocks
             SET status = 'failed', error_code = ?2, safe_error_message = ?3,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ?1 AND status = 'running'",
            params![id.to_string(), error_code, safe_error_message],
        )?;
        if changed != 1 {
            return Err(CoreError::InvalidTransition);
        }
        drop(connection);
        self.get(id)
    }

    /// Atomically persists a completed block and applies its translations.
    pub fn complete(
        &self,
        id: Uuid,
        result: &TranslationResult,
    ) -> Result<TranslationBlock, CoreError> {
        let block = self.get(id)?;
        if block.status != TranslationBlockStatus::Running {
            return Err(CoreError::InvalidTransition);
        }
        result.validate_exact(&block.segment_ids)?;
        let result_json =
            serde_json::to_string(result).map_err(|_| CoreError::InvalidTranslationOutput)?;
        let mut connection = self.database.connection()?;
        let transaction = connection.transaction()?;
        for item in &result.translations {
            let changed = transaction.execute(
                "UPDATE segments
                 SET translated_text = ?3, translation_hash = ?4,
                     updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                 WHERE id = ?1 AND project_id = ?2",
                params![
                    item.id.to_string(),
                    block.project_id.to_string(),
                    item.text,
                    compute_text_hash(&item.text),
                ],
            )?;
            if changed != 1 {
                return Err(CoreError::InvalidTranslationOutput);
            }
        }
        let changed = transaction.execute(
            "UPDATE translation_blocks
             SET status = 'completed', result_json = ?2,
                 error_code = NULL, safe_error_message = NULL,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ?1 AND status = 'running'",
            params![id.to_string(), result_json],
        )?;
        if changed != 1 {
            return Err(CoreError::InvalidTransition);
        }
        transaction.commit()?;
        drop(connection);
        self.get(id)
    }

    pub fn upsert_glossary(
        &self,
        project_id: Uuid,
        source_text: &str,
        target_text: &str,
        case_sensitive: bool,
    ) -> Result<GlossaryEntry, CoreError> {
        if source_text.trim().is_empty() || target_text.trim().is_empty() {
            return Err(CoreError::InvalidInput("glossary entry"));
        }
        let id = Uuid::new_v4();
        let connection = self.database.connection()?;
        connection.execute(
            "INSERT INTO glossary_entries(
                id, project_id, source_text, target_text, case_sensitive, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5,
                strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
             ON CONFLICT(project_id, source_text) DO UPDATE SET
                target_text = excluded.target_text,
                case_sensitive = excluded.case_sensitive,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
            params![
                id.to_string(),
                project_id.to_string(),
                source_text.trim(),
                target_text.trim(),
                case_sensitive as i32,
            ],
        )?;
        drop(connection);
        self.list_glossary(project_id)?
            .into_iter()
            .find(|entry| entry.source_text == source_text.trim())
            .ok_or(CoreError::NotFound("glossary entry"))
    }

    pub fn list_glossary(&self, project_id: Uuid) -> Result<Vec<GlossaryEntry>, CoreError> {
        let connection = self.database.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, project_id, source_text, target_text, case_sensitive
             FROM glossary_entries WHERE project_id = ?1 ORDER BY source_text",
        )?;
        let values = statement
            .query_map([project_id.to_string()], |row| {
                Ok(GlossaryEntry {
                    id: parse_uuid(&row.get::<_, String>(0)?)?,
                    project_id: parse_uuid(&row.get::<_, String>(1)?)?,
                    source_text: row.get(2)?,
                    target_text: row.get(3)?,
                    case_sensitive: row.get(4)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(values)
    }

    pub fn add_locked_name(
        &self,
        project_id: Uuid,
        value: &str,
    ) -> Result<LockedProperName, CoreError> {
        if value.trim().is_empty() {
            return Err(CoreError::InvalidInput("locked proper name"));
        }
        let id = Uuid::new_v4();
        let connection = self.database.connection()?;
        connection.execute(
            "INSERT INTO locked_proper_names(id, project_id, value, created_at, updated_at)
             VALUES (?1, ?2, ?3,
                strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
             ON CONFLICT(project_id, value) DO NOTHING",
            params![id.to_string(), project_id.to_string(), value.trim()],
        )?;
        drop(connection);
        self.list_locked_names(project_id)?
            .into_iter()
            .find(|name| name.value == value.trim())
            .ok_or(CoreError::NotFound("locked proper name"))
    }

    pub fn list_locked_names(&self, project_id: Uuid) -> Result<Vec<LockedProperName>, CoreError> {
        let connection = self.database.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, project_id, value FROM locked_proper_names
             WHERE project_id = ?1 ORDER BY value",
        )?;
        let values = statement
            .query_map([project_id.to_string()], |row| {
                Ok(LockedProperName {
                    id: parse_uuid(&row.get::<_, String>(0)?)?,
                    project_id: parse_uuid(&row.get::<_, String>(1)?)?,
                    value: row.get(2)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(values)
    }
}

fn select_blocks(clause: &str) -> String {
    format!(
        "SELECT id, project_id, stage_run_id, block_index, segment_ids_json,
                source_hash, status, attempts, result_json, error_code,
                safe_error_message, created_at, updated_at
         FROM translation_blocks {clause}"
    )
}

fn block_from_row(row: &Row<'_>) -> rusqlite::Result<TranslationBlock> {
    let result_json: Option<String> = row.get(8)?;
    Ok(TranslationBlock {
        id: parse_uuid(&row.get::<_, String>(0)?)?,
        project_id: parse_uuid(&row.get::<_, String>(1)?)?,
        stage_run_id: parse_uuid(&row.get::<_, String>(2)?)?,
        block_index: row.get(3)?,
        segment_ids: serde_json::from_str(&row.get::<_, String>(4)?)
            .map_err(|_| rusqlite::Error::InvalidQuery)?,
        source_hash: row.get(5)?,
        status: TranslationBlockStatus::from_storage(&row.get::<_, String>(6)?)
            .map_err(|_| rusqlite::Error::InvalidQuery)?,
        attempts: row.get(7)?,
        result: result_json
            .map(|value| serde_json::from_str(&value))
            .transpose()
            .map_err(|_| rusqlite::Error::InvalidQuery)?,
        error_code: row.get(9)?,
        safe_error_message: row.get(10)?,
        created_at: row.get(11)?,
        updated_at: row.get(12)?,
    })
}

fn parse_uuid(value: &str) -> rusqlite::Result<Uuid> {
    Uuid::parse_str(value).map_err(|_| rusqlite::Error::InvalidQuery)
}
