use rusqlite::{params, OptionalExtension, Row};

use crate::domain::{CoreError, ModelConsent, ModelManifest};

use super::Database;

#[derive(Clone)]
pub struct ModelConsentRepository {
    database: Database,
}

impl ModelConsentRepository {
    pub fn new(database: Database) -> Self {
        Self { database }
    }

    pub fn insert_consent(&self, manifest: &ModelManifest) -> Result<ModelConsent, CoreError> {
        manifest.validate()?;
        let connection = self.database.connection()?;
        connection.execute(
            "INSERT OR REPLACE INTO model_consents(
                model_id, provider, display_name, license,
                sends_data_off_device, estimated_size_bytes,
                consented_at, created_at
             ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6,
                strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             )",
            params![
                manifest.model_id,
                manifest.provider,
                manifest.display_name,
                manifest.license,
                manifest.sends_data_off_device as i32,
                manifest.estimated_size_bytes as i64,
            ],
        )?;
        drop(connection);
        self.get(&manifest.model_id)?
            .ok_or(CoreError::NotFound("model consent"))
    }

    pub fn get(&self, model_id: &str) -> Result<Option<ModelConsent>, CoreError> {
        let connection = self.database.connection()?;
        let consent = connection
            .query_row(
                "SELECT model_id, provider, display_name, license,
                        sends_data_off_device, estimated_size_bytes,
                        consented_at, created_at
                 FROM model_consents WHERE model_id = ?1",
                [model_id],
                consent_from_row,
            )
            .optional()?;
        Ok(consent)
    }

    pub fn has_consent(&self, model_id: &str) -> Result<bool, CoreError> {
        let connection = self.database.connection()?;
        let exists: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM model_consents WHERE model_id = ?1)",
            [model_id],
            |row| row.get(0),
        )?;
        Ok(exists)
    }

    pub fn list_all(&self) -> Result<Vec<ModelConsent>, CoreError> {
        let connection = self.database.connection()?;
        let mut statement = connection.prepare(
            "SELECT model_id, provider, display_name, license,
                    sends_data_off_device, estimated_size_bytes,
                    consented_at, created_at
             FROM model_consents ORDER BY consented_at DESC",
        )?;
        let consents = statement
            .query_map([], consent_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(consents)
    }
}

fn consent_from_row(row: &Row<'_>) -> rusqlite::Result<ModelConsent> {
    let sends_off_device: i32 = row.get(4)?;
    let size_bytes: i64 = row.get(5)?;
    Ok(ModelConsent {
        model_id: row.get(0)?,
        provider: row.get(1)?,
        display_name: row.get(2)?,
        license: row.get(3)?,
        sends_data_off_device: sends_off_device != 0,
        estimated_size_bytes: size_bytes as u64,
        consented_at: row.get(6)?,
        created_at: row.get(7)?,
    })
}
