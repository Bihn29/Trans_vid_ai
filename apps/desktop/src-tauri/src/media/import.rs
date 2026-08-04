use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::Path,
};

use serde_json::{Map, Value};
use uuid::Uuid;

use crate::{
    domain::{Artifact, ArtifactKind, CoreError, MediaSite, StageName},
    infrastructure::{ArtifactRegistry, ProjectLayout, ProjectRelativePath, ProjectService},
};

const COPY_BUFFER_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy)]
pub struct MediaImportLimits {
    pub max_source_bytes: u64,
}

impl MediaImportLimits {
    pub fn new(max_source_bytes: u64) -> Result<Self, CoreError> {
        if max_source_bytes == 0 || max_source_bytes > 1024 * 1024 * 1024 * 1024 {
            return Err(CoreError::InvalidInput("media import limit"));
        }
        Ok(Self { max_source_bytes })
    }
}

#[derive(Clone)]
pub struct MediaImportService {
    projects: ProjectService,
    artifacts: ArtifactRegistry,
    layout: ProjectLayout,
    limits: MediaImportLimits,
}

impl MediaImportService {
    pub fn new(
        projects: ProjectService,
        artifacts: ArtifactRegistry,
        layout: ProjectLayout,
        limits: MediaImportLimits,
    ) -> Self {
        Self {
            projects,
            artifacts,
            layout,
            limits,
        }
    }

    pub fn import_local(&self, project_id: Uuid, source: &Path) -> Result<Artifact, CoreError> {
        self.projects.get(project_id)?;
        let mut metadata = Map::new();
        metadata.insert("origin".into(), Value::String("local".into()));
        self.import_from_path(project_id, source, metadata)
    }

    pub fn promote_remote_download(
        &self,
        project_id: Uuid,
        staged_relative_path: &str,
        site: MediaSite,
    ) -> Result<Artifact, CoreError> {
        self.projects.get(project_id)?;
        let relative = ProjectRelativePath::parse(staged_relative_path)?;
        if !relative.as_str().starts_with("temp/") {
            return Err(CoreError::UnsafePath);
        }
        let source = self.layout.resolve_existing(project_id, &relative)?;
        let mut metadata = Map::new();
        metadata.insert("origin".into(), Value::String("remote".into()));
        metadata.insert("site".into(), Value::String(site.as_str().into()));
        let artifact = self.import_from_path(project_id, &source, metadata)?;
        remove_read_only_file(&source);
        Ok(artifact)
    }

    fn import_from_path(
        &self,
        project_id: Uuid,
        source: &Path,
        metadata: Map<String, Value>,
    ) -> Result<Artifact, CoreError> {
        if self.projects.get(project_id)?.source_asset_id.is_some() {
            return Err(CoreError::SourceAlreadySet);
        }
        if !source.is_absolute() {
            return Err(CoreError::UnsafePath);
        }
        let source_metadata = fs::symlink_metadata(source)?;
        if source_metadata.file_type().is_symlink() || !source_metadata.is_file() {
            return Err(CoreError::UnsafePath);
        }
        if source_metadata.len() == 0 {
            return Err(CoreError::UnsupportedMedia);
        }
        if source_metadata.len() > self.limits.max_source_bytes {
            return Err(CoreError::SourceTooLarge);
        }
        let source = source.canonicalize()?;
        let extension = media_extension(&source)?;
        let relative = ProjectRelativePath::parse(format!("source/original.{extension}"))?;
        let destination = self.layout.prepare_output(project_id, &relative)?;
        let temporary =
            destination.with_extension(format!("{extension}.{}.partial", Uuid::new_v4()));

        let copy_result = copy_bounded(&source, &temporary, self.limits.max_source_bytes)
            .and_then(|_| fs::rename(&temporary, &destination).map_err(CoreError::from));
        if let Err(error) = copy_result {
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
        if let Err(error) = make_read_only(&destination) {
            let _ = fs::remove_file(&destination);
            return Err(error);
        }

        let artifact = match self.artifacts.register_existing(
            project_id,
            ArtifactKind::SourceVideo,
            relative.as_str(),
            StageName::Import,
            &metadata,
        ) {
            Ok(artifact) => artifact,
            Err(error) => {
                remove_read_only_file(&destination);
                return Err(error);
            }
        };
        if let Err(error) = self.projects.attach_source_asset(project_id, artifact.id) {
            let _ = self.artifacts.unregister(artifact.id);
            remove_read_only_file(&destination);
            return Err(error);
        }
        Ok(artifact)
    }
}

fn media_extension(path: &Path) -> Result<String, CoreError> {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        .ok_or(CoreError::UnsupportedMedia)?;
    if !matches!(extension.as_str(), "mp4" | "mov" | "mkv" | "webm") {
        return Err(CoreError::UnsupportedMedia);
    }
    Ok(extension)
}

fn copy_bounded(source: &Path, destination: &Path, max_bytes: u64) -> Result<(), CoreError> {
    let mut input = File::open(source)?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)?;
    let mut copied = 0_u64;
    let mut buffer = vec![0_u8; COPY_BUFFER_BYTES];
    loop {
        let read = input.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        copied = copied
            .checked_add(read as u64)
            .ok_or(CoreError::SourceTooLarge)?;
        if copied > max_bytes {
            return Err(CoreError::SourceTooLarge);
        }
        output.write_all(&buffer[..read])?;
    }
    output.sync_all()?;
    Ok(())
}

fn make_read_only(path: &Path) -> Result<(), CoreError> {
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_readonly(true);
    fs::set_permissions(path, permissions)?;
    Ok(())
}

fn remove_read_only_file(path: &Path) {
    if let Ok(metadata) = fs::metadata(path) {
        let mut permissions = metadata.permissions();
        make_owner_writable(&mut permissions);
        let _ = fs::set_permissions(path, permissions);
    }
    let _ = fs::remove_file(path);
}

#[cfg(windows)]
fn make_owner_writable(permissions: &mut fs::Permissions) {
    permissions.set_readonly(false);
}

#[cfg(unix)]
fn make_owner_writable(permissions: &mut fs::Permissions) {
    use std::os::unix::fs::PermissionsExt;

    permissions.set_mode(permissions.mode() | 0o200);
}
