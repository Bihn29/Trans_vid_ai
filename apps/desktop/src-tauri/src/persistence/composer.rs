use rusqlite::{params, OptionalExtension};
use uuid::Uuid;

use super::Database;
use crate::domain::{ComposerConfig, CoreError};

#[derive(Clone)]
pub struct ComposerRepository {
    database: Database,
}

impl ComposerRepository {
    pub fn new(database: Database) -> Self {
        Self { database }
    }

    pub fn get_config(&self, project_id: Uuid) -> Result<ComposerConfig, CoreError> {
        let connection = self.database.connection()?;
        let json: Option<String> = connection
            .query_row(
                "SELECT config_json FROM composer_configs WHERE project_id=?1",
                [project_id.to_string()],
                |row| row.get(0),
            )
            .optional()?;
        match json {
            Some(value) => serde_json::from_str(&value)
                .map_err(|_| CoreError::InvalidInput("stored composer config")),
            None => Ok(ComposerConfig::defaults(project_id)),
        }
    }

    pub fn save_config(&self, config: &ComposerConfig) -> Result<ComposerConfig, CoreError> {
        config.validate()?;
        let encoded = serde_json::to_string(config)
            .map_err(|_| CoreError::InvalidInput("composer config"))?;
        if encoded.len() > 65_536 {
            return Err(CoreError::InvalidInput("composer config size"));
        }
        let connection = self.database.connection()?;
        let exists: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM projects WHERE id=?1)",
            [config.project_id.to_string()],
            |row| row.get(0),
        )?;
        if !exists {
            return Err(CoreError::NotFound("project"));
        }
        connection.execute(
            "INSERT INTO composer_configs(project_id,config_json,updated_at)
             VALUES(?1,?2,strftime('%Y-%m-%dT%H:%M:%fZ','now'))
             ON CONFLICT(project_id) DO UPDATE SET
                config_json=excluded.config_json,updated_at=excluded.updated_at",
            params![config.project_id.to_string(), encoded],
        )?;
        drop(connection);
        self.get_config(config.project_id)
    }
}
