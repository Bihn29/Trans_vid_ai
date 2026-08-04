use std::{
    path::Path,
    sync::{Arc, Mutex, MutexGuard},
};

use rusqlite::Connection;

use crate::domain::CoreError;

use super::{apply_migrations, MigrationError};

#[derive(Clone)]
pub struct Database {
    connection: Arc<Mutex<Connection>>,
}

impl Database {
    pub fn open(path: &Path) -> Result<Self, CoreError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut connection = Connection::open(path)?;
        configure(&connection)?;
        apply_migrations(&mut connection).map_err(map_migration_error)?;
        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
        })
    }

    pub fn in_memory() -> Result<Self, CoreError> {
        let mut connection = Connection::open_in_memory()?;
        configure(&connection)?;
        apply_migrations(&mut connection).map_err(map_migration_error)?;
        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
        })
    }

    pub(crate) fn connection(&self) -> Result<MutexGuard<'_, Connection>, CoreError> {
        self.connection.lock().map_err(|_| CoreError::LockPoisoned)
    }

    pub fn schema_version(&self) -> Result<i64, CoreError> {
        Ok(self.connection()?.query_row(
            "SELECT MAX(version) FROM schema_migrations",
            [],
            |row| row.get(0),
        )?)
    }
}

fn configure(connection: &Connection) -> Result<(), CoreError> {
    connection.execute_batch(
        "PRAGMA foreign_keys = ON;
         PRAGMA busy_timeout = 5000;",
    )?;
    Ok(())
}

fn map_migration_error(error: MigrationError) -> CoreError {
    match error {
        MigrationError::Database(error) => CoreError::Database(error),
        MigrationError::DatabaseNewer { .. } => CoreError::InvalidInput("database schema version"),
    }
}
