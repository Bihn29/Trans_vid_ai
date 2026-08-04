use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use crate::{domain::CoreError, persistence::Database};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrivacySettings {
    pub metadata_logging_enabled: bool,
    pub max_log_files: u8,
    pub max_log_file_bytes: u64,
}

impl PrivacySettings {
    pub fn validate(self) -> Result<Self, CoreError> {
        if !(1..=10).contains(&self.max_log_files)
            || !(64 * 1024..=10 * 1024 * 1024).contains(&self.max_log_file_bytes)
        {
            return Err(CoreError::InvalidInput("privacy settings"));
        }
        Ok(self)
    }

    pub fn load(database: &Database) -> Result<Self, CoreError> {
        let connection = database.connection()?;
        connection
            .query_row(
                "SELECT metadata_logging_enabled,max_log_files,max_log_file_bytes
                 FROM privacy_settings WHERE singleton=1",
                [],
                |row| {
                    Ok(Self {
                        metadata_logging_enabled: row.get::<_, i64>(0)? != 0,
                        max_log_files: row.get(1)?,
                        max_log_file_bytes: row.get(2)?,
                    })
                },
            )
            .map_err(Into::into)
    }

    pub fn save(self, database: &Database) -> Result<Self, CoreError> {
        let value = self.validate()?;
        database.connection()?.execute(
            "UPDATE privacy_settings SET metadata_logging_enabled=?1,max_log_files=?2,
             max_log_file_bytes=?3,updated_at=strftime('%Y-%m-%dT%H:%M:%fZ','now')
             WHERE singleton=1",
            rusqlite::params![
                value.metadata_logging_enabled as i64,
                value.max_log_files,
                value.max_log_file_bytes
            ],
        )?;
        Ok(value)
    }
}

#[derive(Debug, Clone)]
pub struct PrivacyLog {
    root: PathBuf,
    settings: PrivacySettings,
}

#[derive(Clone)]
pub struct PrivacyService {
    database: Database,
    log_root: PathBuf,
}

impl PrivacyService {
    pub fn new(database: Database, log_root: PathBuf) -> Result<Self, CoreError> {
        PrivacyLog::new(&log_root, PrivacySettings::load(&database)?)?;
        Ok(Self { database, log_root })
    }

    pub fn settings(&self) -> Result<PrivacySettings, CoreError> {
        PrivacySettings::load(&self.database)
    }

    pub fn save_settings(&self, settings: PrivacySettings) -> Result<PrivacySettings, CoreError> {
        settings.save(&self.database)
    }

    pub fn write_event(
        &self,
        code: &str,
        fields: &BTreeMap<String, String>,
    ) -> Result<(), CoreError> {
        PrivacyLog::new(&self.log_root, self.settings()?)?.write_event(code, fields)
    }

    pub fn log_root(&self) -> &Path {
        &self.log_root
    }
}

impl PrivacyLog {
    pub fn new(root: impl AsRef<Path>, settings: PrivacySettings) -> Result<Self, CoreError> {
        let settings = settings.validate()?;
        fs::create_dir_all(root.as_ref())?;
        let metadata = fs::symlink_metadata(root.as_ref())?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(CoreError::UnsafePath);
        }
        Ok(Self {
            root: root.as_ref().canonicalize()?,
            settings,
        })
    }

    pub fn write_event(
        &self,
        code: &str,
        fields: &BTreeMap<String, String>,
    ) -> Result<(), CoreError> {
        if !self.settings.metadata_logging_enabled {
            return Ok(());
        }
        validate_event_code(code)?;
        if fields.len() > 16
            || fields.keys().any(|key| {
                key.is_empty()
                    || key.len() > 48
                    || !key
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            })
        {
            return Err(CoreError::InvalidInput("privacy log fields"));
        }
        let path = self.root.join("vietdub.log");
        let safe_fields = fields
            .iter()
            .map(|(key, value)| (key.clone(), redact(value)))
            .collect::<BTreeMap<_, _>>();
        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let mut record = serde_json::to_vec(&serde_json::json!({
            "timestampMs": timestamp_ms,
            "event": code,
            "fields": safe_fields,
        }))
        .map_err(|_| CoreError::InvalidInput("privacy log record"))?;
        record.push(b'\n');
        if record.len() as u64 > self.settings.max_log_file_bytes {
            return Err(CoreError::InvalidInput("privacy log record size"));
        }
        let existing = fs::metadata(&path).map_or(0, |metadata| metadata.len());
        if existing.saturating_add(record.len() as u64) > self.settings.max_log_file_bytes {
            self.rotate()?;
        }
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        file.write_all(&record)?;
        file.sync_data()?;
        Ok(())
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn rotate(&self) -> Result<(), CoreError> {
        let current = self.root.join("vietdub.log");
        if self.settings.max_log_files == 1 {
            if current.exists() {
                fs::remove_file(current)?;
            }
            return Ok(());
        }
        let oldest = self
            .root
            .join(format!("vietdub.{}.log", self.settings.max_log_files - 1));
        if oldest.exists() {
            fs::remove_file(oldest)?;
        }
        for index in (1..self.settings.max_log_files - 1).rev() {
            let source = self.root.join(format!("vietdub.{index}.log"));
            let destination = self.root.join(format!("vietdub.{}.log", index + 1));
            if source.exists() {
                fs::rename(source, destination)?;
            }
        }
        if current.exists() {
            fs::rename(current, self.root.join("vietdub.1.log"))?;
        }
        Ok(())
    }
}

fn validate_event_code(value: &str) -> Result<(), CoreError> {
    if value.len() < 3
        || value.len() > 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
    {
        return Err(CoreError::InvalidInput("privacy event code"));
    }
    Ok(())
}

fn redact(value: &str) -> String {
    let lower = value.to_ascii_lowercase();
    if lower.contains("authorization")
        || lower.contains("api_key")
        || lower.contains("api-key")
        || lower.contains("secret")
        || lower.contains("bearer ")
        || value.contains("sk-")
    {
        return "[REDACTED]".into();
    }
    if Path::new(value).is_absolute() || value.contains("\\Users\\") || value.contains("/home/") {
        return "[PATH]".into();
    }
    value.chars().take(256).collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PerformanceBudget {
    pub startup: Duration,
    pub queue_recovery: Duration,
    pub artifact_hash_64_mib: Duration,
    pub ui_interaction: Duration,
}

impl Default for PerformanceBudget {
    fn default() -> Self {
        Self {
            startup: Duration::from_secs(5),
            queue_recovery: Duration::from_secs(2),
            artifact_hash_64_mib: Duration::from_secs(3),
            ui_interaction: Duration::from_millis(100),
        }
    }
}

impl PerformanceBudget {
    pub fn validate(self) -> Result<Self, CoreError> {
        if self.startup.is_zero()
            || self.queue_recovery.is_zero()
            || self.artifact_hash_64_mib.is_zero()
            || self.ui_interaction.is_zero()
        {
            return Err(CoreError::InvalidInput("performance budget"));
        }
        Ok(self)
    }
}
