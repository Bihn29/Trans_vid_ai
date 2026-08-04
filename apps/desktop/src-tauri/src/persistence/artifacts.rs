use rusqlite::{params, OptionalExtension, Row};
use uuid::Uuid;

use crate::domain::{Artifact, ArtifactKind, CoreError, NewArtifact};

use super::Database;

#[derive(Clone)]
pub struct ArtifactRepository {
    database: Database,
}

impl ArtifactRepository {
    pub fn new(database: Database) -> Self {
        Self { database }
    }

    pub fn insert(&self, artifact: &NewArtifact) -> Result<Artifact, CoreError> {
        artifact.validate()?;
        let metadata = serde_json::to_string(&artifact.metadata)
            .map_err(|_| CoreError::InvalidInput("artifact metadata"))?;
        let size_bytes = i64::try_from(artifact.size_bytes)
            .map_err(|_| CoreError::InvalidInput("artifact size"))?;
        let connection = self.database.connection()?;
        connection.execute(
            "INSERT INTO artifacts(
                id, project_id, type, relative_path, sha256, size_bytes,
                created_at, producer_stage, metadata
             ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6,
                strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), ?7, ?8
             )",
            params![
                artifact.id.to_string(),
                artifact.project_id.to_string(),
                artifact.kind.as_str(),
                artifact.relative_path,
                artifact.sha256,
                size_bytes,
                artifact.producer_stage,
                metadata,
            ],
        )?;
        drop(connection);
        self.get(artifact.id)
    }

    pub fn get(&self, id: Uuid) -> Result<Artifact, CoreError> {
        let connection = self.database.connection()?;
        connection
            .query_row(
                "SELECT id, project_id, type, relative_path, sha256, size_bytes,
                        created_at, producer_stage, metadata
                 FROM artifacts WHERE id = ?1",
                [id.to_string()],
                artifact_from_row,
            )
            .optional()?
            .ok_or(CoreError::NotFound("artifact"))
    }

    pub fn list_for_project(&self, project_id: Uuid) -> Result<Vec<Artifact>, CoreError> {
        let connection = self.database.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, project_id, type, relative_path, sha256, size_bytes,
                    created_at, producer_stage, metadata
             FROM artifacts WHERE project_id = ?1 ORDER BY created_at, id",
        )?;
        let artifacts = statement
            .query_map([project_id.to_string()], artifact_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(artifacts)
    }

    pub fn delete(&self, id: Uuid) -> Result<(), CoreError> {
        let connection = self.database.connection()?;
        let changed =
            connection.execute("DELETE FROM artifacts WHERE id = ?1", [id.to_string()])?;
        if changed != 1 {
            return Err(CoreError::NotFound("artifact"));
        }
        Ok(())
    }
}

fn artifact_from_row(row: &Row<'_>) -> rusqlite::Result<Artifact> {
    let id: String = row.get(0)?;
    let project_id: String = row.get(1)?;
    let kind: String = row.get(2)?;
    let size_bytes: i64 = row.get(5)?;
    let metadata: String = row.get(8)?;
    Ok(Artifact {
        id: parse_uuid(&id)?,
        project_id: parse_uuid(&project_id)?,
        kind: ArtifactKind::from_storage(&kind).map_err(|_| rusqlite::Error::InvalidQuery)?,
        relative_path: row.get(3)?,
        sha256: row.get(4)?,
        size_bytes: u64::try_from(size_bytes).map_err(|_| rusqlite::Error::InvalidQuery)?,
        created_at: row.get(6)?,
        producer_stage: row.get(7)?,
        metadata: serde_json::from_str(&metadata).map_err(|_| rusqlite::Error::InvalidQuery)?,
    })
}

fn parse_uuid(value: &str) -> rusqlite::Result<Uuid> {
    Uuid::parse_str(value).map_err(|_| rusqlite::Error::InvalidQuery)
}
