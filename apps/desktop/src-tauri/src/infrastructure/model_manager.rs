use std::{
    fs,
    path::{Path, PathBuf},
};

use sha2::{Digest, Sha256};

use crate::{
    domain::{ApprovedModelManifest, CoreError, ModelInstallationReport},
    persistence::Database,
};

use super::{sha256_file, ProjectRelativePath};

const MAX_CATALOG_MANIFEST_BYTES: u64 = 64 * 1024;
const MAX_INSTALL_MANIFEST_BYTES: u64 = 1024 * 1024;
const MAX_MODEL_FILES: usize = 4096;

#[derive(Debug, serde::Deserialize)]
struct InstalledModelManifest {
    schema_version: u32,
    model_id: String,
    provider: String,
    version: String,
    license: String,
    source_url: String,
    files: Vec<InstalledModelFile>,
}

#[derive(Debug, serde::Deserialize)]
struct InstalledModelFile {
    relative_path: String,
    sha256: String,
    size_bytes: u64,
}

#[derive(Clone)]
pub struct ModelManager {
    database: Database,
    catalog: Vec<ApprovedModelManifest>,
}

impl ModelManager {
    pub fn load(database: Database, catalog_root: &Path) -> Result<Self, CoreError> {
        let metadata = fs::symlink_metadata(catalog_root)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(CoreError::UnsafePath);
        }
        let root = catalog_root.canonicalize()?;
        let mut paths = fs::read_dir(&root)?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|value| value == "json"))
            .collect::<Vec<_>>();
        paths.sort();
        if paths.is_empty() || paths.len() > 32 {
            return Err(CoreError::InvalidInput("model catalog"));
        }
        let mut catalog = Vec::with_capacity(paths.len());
        for path in paths {
            let metadata = fs::symlink_metadata(&path)?;
            let canonical = path.canonicalize()?;
            if metadata.file_type().is_symlink()
                || !metadata.is_file()
                || metadata.len() > MAX_CATALOG_MANIFEST_BYTES
                || !canonical.starts_with(&root)
            {
                return Err(CoreError::ArtifactIntegrity);
            }
            let manifest: ApprovedModelManifest = serde_json::from_slice(&fs::read(canonical)?)
                .map_err(|_| CoreError::InvalidInput("model catalog manifest"))?;
            manifest.validate()?;
            if catalog
                .iter()
                .any(|value: &ApprovedModelManifest| value.model_id == manifest.model_id)
            {
                return Err(CoreError::Conflict("model catalog id"));
            }
            catalog.push(manifest);
        }
        Ok(Self { database, catalog })
    }

    pub fn list(&self) -> Vec<ApprovedModelManifest> {
        self.catalog.clone()
    }

    pub fn get(&self, model_id: &str) -> Result<&ApprovedModelManifest, CoreError> {
        self.catalog
            .iter()
            .find(|manifest| manifest.model_id == model_id)
            .ok_or(CoreError::NotFound("approved model manifest"))
    }

    pub fn verify_installation(
        &self,
        model_id: &str,
        installation_root: &Path,
    ) -> Result<ModelInstallationReport, CoreError> {
        let approved = self.get(model_id)?;
        if !approved.approved_for_local_use {
            return Err(CoreError::InvalidInput(
                "model is not approved for local use",
            ));
        }
        let root_metadata = fs::symlink_metadata(installation_root)?;
        if root_metadata.file_type().is_symlink() || !root_metadata.is_dir() {
            return Err(CoreError::UnsafePath);
        }
        let root = installation_root.canonicalize()?;
        let manifest_path = root.join("vietdub-model.json");
        let metadata = fs::symlink_metadata(&manifest_path)?;
        if metadata.file_type().is_symlink()
            || !metadata.is_file()
            || metadata.len() > MAX_INSTALL_MANIFEST_BYTES
        {
            return Err(CoreError::ArtifactIntegrity);
        }
        let manifest_bytes = fs::read(&manifest_path)?;
        let manifest: InstalledModelManifest = serde_json::from_slice(&manifest_bytes)
            .map_err(|_| CoreError::InvalidInput("installed model manifest"))?;
        if manifest.schema_version != 1
            || manifest.model_id != approved.model_id
            || manifest.provider != approved.provider
            || manifest.version != approved.version
            || manifest.license != approved.license
            || manifest.source_url != approved.source_url
            || manifest.files.is_empty()
            || manifest.files.len() > MAX_MODEL_FILES
        {
            return Err(CoreError::InvalidInput("installed model manifest"));
        }
        let mut total_size_bytes = 0_u64;
        for file in &manifest.files {
            verify_file(&root, file)?;
            total_size_bytes = total_size_bytes
                .checked_add(file.size_bytes)
                .ok_or(CoreError::InvalidInput("installed model size"))?;
        }
        if total_size_bytes == 0 {
            return Err(CoreError::InvalidInput("installed model size"));
        }
        let report = ModelInstallationReport {
            model_id: manifest.model_id,
            version: manifest.version,
            manifest_sha256: format!("{:x}", Sha256::digest(&manifest_bytes)),
            file_count: manifest.files.len(),
            total_size_bytes,
        };
        self.database.connection()?.execute(
            "INSERT INTO verified_model_installations(
                model_id,version,manifest_sha256,file_count,total_size_bytes,verified_at
             ) VALUES(?1,?2,?3,?4,?5,strftime('%Y-%m-%dT%H:%M:%fZ','now'))
             ON CONFLICT(model_id) DO UPDATE SET version=excluded.version,
                manifest_sha256=excluded.manifest_sha256,file_count=excluded.file_count,
                total_size_bytes=excluded.total_size_bytes,verified_at=excluded.verified_at",
            rusqlite::params![
                report.model_id,
                report.version,
                report.manifest_sha256,
                report.file_count,
                report.total_size_bytes,
            ],
        )?;
        Ok(report)
    }
}

fn verify_file(root: &Path, file: &InstalledModelFile) -> Result<(), CoreError> {
    let relative = ProjectRelativePath::parse(&file.relative_path)?;
    if file.size_bytes == 0
        || file.sha256.len() != 64
        || !file
            .sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(CoreError::InvalidInput("installed model file"));
    }
    let candidate = root.join(relative.as_str().split('/').collect::<PathBuf>());
    let metadata = fs::symlink_metadata(&candidate)?;
    let canonical = candidate.canonicalize()?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || !canonical.starts_with(root)
        || metadata.len() != file.size_bytes
    {
        return Err(CoreError::ArtifactIntegrity);
    }
    let (sha256, size) = sha256_file(&canonical)?;
    if sha256 != file.sha256 || size != file.size_bytes {
        return Err(CoreError::ArtifactIntegrity);
    }
    Ok(())
}
