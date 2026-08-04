use std::{fs, path::Path, sync::Arc};

use serde::Serialize;
use uuid::Uuid;

use crate::{domain::CoreError, infrastructure::ProjectLayout, persistence::Database};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoverySummary {
    pub interrupted_sessions: usize,
    pub partial_files_removed: usize,
}

pub struct RuntimeSessionGuard {
    database: Arc<Database>,
    session_id: Uuid,
    summary: RecoverySummary,
}

impl RuntimeSessionGuard {
    pub fn begin(database: Database, layout: &ProjectLayout) -> Result<Self, CoreError> {
        let session_id = Uuid::new_v4();
        let mut connection = database.connection()?;
        let transaction = connection.transaction()?;
        let interrupted_sessions: usize = transaction.query_row(
            "SELECT COUNT(*) FROM runtime_sessions WHERE clean_shutdown=0",
            [],
            |row| row.get(0),
        )?;
        transaction.execute(
            "UPDATE runtime_sessions SET recovered_at=strftime('%Y-%m-%dT%H:%M:%fZ','now')
             WHERE clean_shutdown=0 AND recovered_at IS NULL",
            [],
        )?;
        transaction.execute(
            "INSERT INTO runtime_sessions(session_id,started_at,clean_shutdown)
             VALUES(?1,strftime('%Y-%m-%dT%H:%M:%fZ','now'),0)",
            [session_id.to_string()],
        )?;
        transaction.commit()?;
        drop(connection);

        let partial_files_removed = cleanup_partial_files(layout.root())?;
        Ok(Self {
            database: Arc::new(database),
            session_id,
            summary: RecoverySummary {
                interrupted_sessions,
                partial_files_removed,
            },
        })
    }

    pub fn summary(&self) -> RecoverySummary {
        self.summary
    }

    pub fn session_id(&self) -> Uuid {
        self.session_id
    }

    pub fn finish(&self) -> Result<(), CoreError> {
        self.database.connection()?.execute(
            "UPDATE runtime_sessions SET clean_shutdown=1,
             ended_at=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE session_id=?1",
            [self.session_id.to_string()],
        )?;
        Ok(())
    }

    pub fn is_clean(&self) -> Result<bool, CoreError> {
        Ok(self.database.connection()?.query_row(
            "SELECT clean_shutdown FROM runtime_sessions WHERE session_id=?1",
            [self.session_id.to_string()],
            |row| row.get(0),
        )?)
    }
}

impl Drop for RuntimeSessionGuard {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}

fn cleanup_partial_files(root: &Path) -> Result<usize, CoreError> {
    let root = root.canonicalize()?;
    cleanup_directory(&root, &root, 0)
}

fn cleanup_directory(root: &Path, directory: &Path, depth: usize) -> Result<usize, CoreError> {
    if depth > 6 {
        return Ok(0);
    }
    let mut removed = 0;
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let metadata = fs::symlink_metadata(entry.path())?;
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            removed += cleanup_directory(root, &entry.path(), depth + 1)?;
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if metadata.is_file() && name.starts_with('.') && name.contains(".partial") {
            let canonical = entry.path().canonicalize()?;
            if canonical.starts_with(root) {
                fs::remove_file(canonical)?;
                removed += 1;
            }
        }
    }
    Ok(removed)
}
