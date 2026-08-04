use rusqlite::{params, Connection, OptionalExtension};
use thiserror::Error;

const MIGRATIONS: &[(i64, &str)] = &[
    (1, include_str!("../../migrations/0001_foundation.sql")),
    (2, include_str!("../../migrations/0002_milestone_one.sql")),
    (3, include_str!("../../migrations/0003_milestone_three.sql")),
    (4, include_str!("../../migrations/0004_milestone_four.sql")),
    (5, include_str!("../../migrations/0005_milestone_five.sql")),
    (6, include_str!("../../migrations/0006_milestone_six.sql")),
    (7, include_str!("../../migrations/0007_milestone_seven.sql")),
    (8, include_str!("../../migrations/0008_milestone_eight.sql")),
];

#[derive(Debug, Error)]
pub enum MigrationError {
    #[error("database migration failed")]
    Database(#[from] rusqlite::Error),
    #[error("database schema version {found} is newer than supported version {supported}")]
    DatabaseNewer { found: i64, supported: i64 },
}

pub fn apply_migrations(connection: &mut Connection) -> Result<(), MigrationError> {
    let transaction = connection.transaction()?;
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY NOT NULL,
            applied_at TEXT NOT NULL
        ) STRICT;",
    )?;

    let latest_supported = MIGRATIONS.last().map_or(0, |(version, _)| *version);
    let latest_applied: Option<i64> = transaction
        .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
            row.get(0)
        })
        .optional()?
        .flatten();

    if latest_applied.is_some_and(|version| version > latest_supported) {
        return Err(MigrationError::DatabaseNewer {
            found: latest_applied.unwrap_or_default(),
            supported: latest_supported,
        });
    }

    for (version, sql) in MIGRATIONS {
        let already_applied: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version = ?1)",
            [version],
            |row| row.get(0),
        )?;

        if already_applied {
            continue;
        }

        transaction.execute_batch(sql)?;
        transaction.execute(
            "INSERT INTO schema_migrations(version, applied_at)
             VALUES (?1, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
            params![version],
        )?;
    }

    transaction.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_are_transactional_and_idempotent() {
        let mut connection = Connection::open_in_memory().expect("open in-memory database");

        apply_migrations(&mut connection).expect("apply initial migration");
        apply_migrations(&mut connection).expect("reapply migrations");

        let versions: i64 = connection
            .query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .expect("count migration rows");
        let metadata_exists: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='app_metadata')",
                [],
                |row| row.get(0),
            )
            .expect("check app_metadata table");
        let composer_exists: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='composer_configs')",
                [],
                |row| row.get(0),
            )
            .expect("check composer config table");

        assert_eq!(versions, 8);
        assert!(metadata_exists);
        assert!(composer_exists);
    }

    #[test]
    fn newer_database_is_rejected() {
        let mut connection = Connection::open_in_memory().expect("open in-memory database");
        connection
            .execute_batch(
                "CREATE TABLE schema_migrations (
                    version INTEGER PRIMARY KEY NOT NULL,
                    applied_at TEXT NOT NULL
                ) STRICT;
                INSERT INTO schema_migrations VALUES (99, '2026-08-01T00:00:00Z');",
            )
            .expect("seed future migration");

        let error = apply_migrations(&mut connection).expect_err("reject newer database");
        assert!(matches!(
            error,
            MigrationError::DatabaseNewer {
                found: 99,
                supported: 8
            }
        ));
    }

    #[test]
    fn upgrades_an_existing_foundation_database() {
        let mut connection = Connection::open_in_memory().expect("open in-memory database");
        connection
            .execute_batch(
                "CREATE TABLE schema_migrations (
                    version INTEGER PRIMARY KEY NOT NULL,
                    applied_at TEXT NOT NULL
                ) STRICT;
                CREATE TABLE app_metadata (
                    key TEXT PRIMARY KEY NOT NULL,
                    value TEXT NOT NULL,
                    updated_at TEXT NOT NULL
                ) STRICT;
                INSERT INTO schema_migrations VALUES (1, '2026-08-01T00:00:00Z');",
            )
            .expect("seed milestone zero database");

        apply_migrations(&mut connection).expect("upgrade through milestone three");

        let version: i64 = connection
            .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .expect("read schema version");
        let jobs_exist: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='jobs')",
                [],
                |row| row.get(0),
            )
            .expect("check jobs table");
        let segments_exist: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='segments')",
                [],
                |row| row.get(0),
            )
            .expect("check segments table");
        assert_eq!(version, 8);
        assert!(jobs_exist);
        assert!(segments_exist);
    }
}
