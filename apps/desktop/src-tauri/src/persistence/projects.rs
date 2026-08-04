use rusqlite::{params, OptionalExtension, Row};
use serde_json::{Map, Value};
use uuid::Uuid;

use crate::domain::{CoreError, NewProject, Project, ProjectStatus, ProjectUpdate, WorkflowMode};

use super::Database;

#[derive(Clone)]
pub struct ProjectRepository {
    database: Database,
}

impl ProjectRepository {
    pub fn new(database: Database) -> Self {
        Self { database }
    }

    pub fn insert(&self, id: Uuid, new_project: &NewProject) -> Result<Project, CoreError> {
        new_project.validate()?;
        let config = serde_json::to_string(&new_project.config_snapshot)
            .map_err(|_| CoreError::InvalidInput("project config"))?;
        let connection = self.database.connection()?;
        connection.execute(
            "INSERT INTO projects(
                id, name, status, source_language, target_language, workflow_mode,
                source_asset_id, config_snapshot, created_at, updated_at
             ) VALUES (
                ?1, ?2, 'draft', ?3, ?4, ?5, NULL, ?6,
                strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             )",
            params![
                id.to_string(),
                new_project.name.trim(),
                new_project.source_language,
                new_project.target_language,
                new_project.workflow_mode.as_str(),
                config,
            ],
        )?;
        drop(connection);
        self.get(id)
    }

    pub fn get(&self, id: Uuid) -> Result<Project, CoreError> {
        let connection = self.database.connection()?;
        connection
            .query_row(
                "SELECT id, name, status, source_language, target_language, workflow_mode,
                        source_asset_id, config_snapshot, created_at, updated_at
                 FROM projects WHERE id = ?1",
                [id.to_string()],
                project_from_row,
            )
            .optional()?
            .ok_or(CoreError::NotFound("project"))
    }

    pub fn list(&self) -> Result<Vec<Project>, CoreError> {
        let connection = self.database.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, name, status, source_language, target_language, workflow_mode,
                    source_asset_id, config_snapshot, created_at, updated_at
             FROM projects ORDER BY updated_at DESC, id ASC",
        )?;
        let projects = statement
            .query_map([], project_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(projects)
    }

    pub fn update(&self, id: Uuid, update: &ProjectUpdate) -> Result<Project, CoreError> {
        update.validate()?;
        let current = self.get(id)?;
        let name = update.name.as_deref().unwrap_or(&current.name).trim();
        let status = update.status.unwrap_or(current.status);
        let source_asset_id = update.source_asset_id.unwrap_or(current.source_asset_id);
        let config = update
            .config_snapshot
            .as_ref()
            .unwrap_or(&current.config_snapshot);
        let config =
            serde_json::to_string(config).map_err(|_| CoreError::InvalidInput("project config"))?;
        let connection = self.database.connection()?;
        let changed = connection.execute(
            "UPDATE projects
             SET name = ?2, status = ?3, source_asset_id = ?4, config_snapshot = ?5,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ?1",
            params![
                id.to_string(),
                name,
                status.as_str(),
                source_asset_id.map(|value| value.to_string()),
                config,
            ],
        )?;
        if changed != 1 {
            return Err(CoreError::NotFound("project"));
        }
        drop(connection);
        self.get(id)
    }

    pub fn delete(&self, id: Uuid) -> Result<(), CoreError> {
        let connection = self.database.connection()?;
        let changed = connection.execute("DELETE FROM projects WHERE id = ?1", [id.to_string()])?;
        if changed != 1 {
            return Err(CoreError::NotFound("project"));
        }
        Ok(())
    }
}

fn project_from_row(row: &Row<'_>) -> rusqlite::Result<Project> {
    let id: String = row.get(0)?;
    let status: String = row.get(2)?;
    let workflow_mode: String = row.get(5)?;
    let source_asset_id: Option<String> = row.get(6)?;
    let config: String = row.get(7)?;
    let config = serde_json::from_str::<Map<String, Value>>(&config)
        .map_err(|_| rusqlite::Error::InvalidQuery)?;
    Ok(Project {
        id: parse_uuid(&id)?,
        name: row.get(1)?,
        status: ProjectStatus::from_storage(&status).map_err(|_| rusqlite::Error::InvalidQuery)?,
        source_language: row.get(3)?,
        target_language: row.get(4)?,
        workflow_mode: WorkflowMode::from_storage(&workflow_mode)
            .map_err(|_| rusqlite::Error::InvalidQuery)?,
        source_asset_id: source_asset_id.as_deref().map(parse_uuid).transpose()?,
        config_snapshot: config,
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
    })
}

fn parse_uuid(value: &str) -> rusqlite::Result<Uuid> {
    Uuid::parse_str(value).map_err(|_| rusqlite::Error::InvalidQuery)
}
