use std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString},
    fs,
    io::{BufReader, Read},
    path::{Component, Path, PathBuf},
    time::Duration,
};

use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::domain::{CoreError, StageName};
use crate::infrastructure::ModelManager;
use crate::persistence::ModelConsentRepository;

use super::client::{WorkerClient, WorkerCommand};

const DEFAULT_WORKER_TIMEOUT: Duration = Duration::from_secs(600);
const DEFAULT_MAX_MESSAGE_BYTES: usize = 4 * 1024 * 1024;
const MODEL_MANIFEST_FILENAME: &str = "vietdub-model.json";
const MAX_MODEL_MANIFEST_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Clone)]
pub struct RequiredModel {
    pub model_id: String,
    pub root: PathBuf,
}

#[derive(Debug, Deserialize)]
struct InstalledModelManifest {
    schema_version: u32,
    model_id: String,
    provider: String,
    version: String,
    license: String,
    source_url: String,
    files: Vec<InstalledModelFile>,
}

#[derive(Debug, Deserialize)]
struct InstalledModelFile {
    relative_path: String,
    sha256: String,
    size_bytes: u64,
}

/// Maps pipeline stages to consent-gated, locally verified Python workers.
#[derive(Clone)]
pub struct WorkerManager {
    python: PathBuf,
    workers_root: PathBuf,
    model_consents: ModelConsentRepository,
    approved_models: Option<ModelManager>,
    environment: BTreeMap<OsString, OsString>,
}

impl WorkerManager {
    pub fn new(
        python: PathBuf,
        workers_root: PathBuf,
        model_consents: ModelConsentRepository,
    ) -> Self {
        Self {
            python,
            workers_root,
            model_consents,
            approved_models: None,
            environment: BTreeMap::new(),
        }
    }

    pub fn with_model_manager(mut self, approved_models: ModelManager) -> Self {
        self.approved_models = Some(approved_models);
        self
    }

    pub fn with_environment(mut self, environment: BTreeMap<OsString, OsString>) -> Self {
        self.environment = environment;
        self
    }

    pub fn client_for_stage(
        &self,
        stage: StageName,
        project_root: &Path,
        required_models: &[RequiredModel],
    ) -> Result<WorkerClient, CoreError> {
        if self.stage_requires_consent(stage) && required_models.is_empty() {
            return Err(CoreError::ModelNotConsented);
        }
        for model in required_models {
            self.require_consent_for_stage(stage, &model.model_id)?;
            if let Some(manager) = &self.approved_models {
                manager.verify_installation(&model.model_id, &model.root)?;
            } else {
                self.verify_installed_model(model)?;
            }
        }
        let project_root = project_root.canonicalize()?;
        if !project_root.is_dir() {
            return Err(CoreError::UnsafePath);
        }
        let command = self
            .command_for_stage(stage)?
            .with_environment(self.environment.clone())
            .with_working_directory(project_root);
        Ok(WorkerClient::new(
            command,
            self.timeout_for_stage(stage),
            DEFAULT_MAX_MESSAGE_BYTES,
        ))
    }

    pub fn require_consent_for_stage(
        &self,
        stage: StageName,
        model_id: &str,
    ) -> Result<(), CoreError> {
        if self.stage_requires_consent(stage) && !self.model_consents.has_consent(model_id)? {
            return Err(CoreError::ModelNotConsented);
        }
        Ok(())
    }

    fn command_for_stage(&self, stage: StageName) -> Result<WorkerCommand, CoreError> {
        let script = match stage {
            StageName::Transcribe => "asr/main.py",
            StageName::Translate => "translation/main.py",
            StageName::Synthesize | StageName::VoicePreview => "tts/main.py",
            StageName::SeparateAudio => "separation/main.py",
            _ => return Err(CoreError::InvalidInput("unsupported worker stage")),
        };
        let workers_root = self.workers_root.canonicalize()?;
        let script_path = workers_root.join(script).canonicalize()?;
        let script_metadata = fs::symlink_metadata(&script_path)?;
        let python_path = self.python.canonicalize()?;
        let python_metadata = fs::symlink_metadata(&python_path)?;
        if !script_path.starts_with(&workers_root)
            || script_metadata.file_type().is_symlink()
            || !script_metadata.is_file()
            || python_metadata.file_type().is_symlink()
            || !python_metadata.is_file()
        {
            return Err(CoreError::UnsafePath);
        }
        Ok(WorkerCommand::new(python_path, script_path))
    }

    fn timeout_for_stage(&self, stage: StageName) -> Duration {
        match stage {
            StageName::Transcribe | StageName::SeparateAudio => Duration::from_secs(1800),
            StageName::Synthesize => Duration::from_secs(3 * 60 * 60),
            _ => DEFAULT_WORKER_TIMEOUT,
        }
    }

    fn stage_requires_consent(&self, stage: StageName) -> bool {
        matches!(stage, StageName::Transcribe)
    }

    fn verify_installed_model(&self, required: &RequiredModel) -> Result<(), CoreError> {
        let consent = self
            .model_consents
            .get(&required.model_id)?
            .ok_or(CoreError::ModelNotConsented)?;
        let root_metadata = fs::symlink_metadata(&required.root)?;
        if root_metadata.file_type().is_symlink() || !root_metadata.is_dir() {
            return Err(CoreError::UnsafePath);
        }
        let root = required.root.canonicalize()?;
        let manifest_path = root.join(MODEL_MANIFEST_FILENAME);
        let manifest_metadata = fs::symlink_metadata(&manifest_path)?;
        if manifest_metadata.file_type().is_symlink()
            || !manifest_metadata.is_file()
            || manifest_metadata.len() > MAX_MODEL_MANIFEST_BYTES
        {
            return Err(CoreError::ArtifactIntegrity);
        }
        let manifest: InstalledModelManifest =
            serde_json::from_slice(&fs::read(&manifest_path)?)
                .map_err(|_| CoreError::InvalidInput("installed model manifest"))?;
        if manifest.schema_version != 1
            || manifest.model_id != required.model_id
            || manifest.provider != consent.provider
            || manifest.license != consent.license
            || manifest.version.is_empty()
            || manifest.version.len() > 128
            || manifest.files.is_empty()
            || url::Url::parse(&manifest.source_url)
                .ok()
                .is_none_or(|url| url.scheme() != "https" || url.host_str().is_none())
        {
            return Err(CoreError::InvalidInput("installed model manifest"));
        }
        for file in &manifest.files {
            verify_model_file(&root, file)?;
        }
        Ok(())
    }
}

fn verify_model_file(root: &Path, file: &InstalledModelFile) -> Result<(), CoreError> {
    if !is_safe_model_relative_path(&file.relative_path)
        || file.sha256.len() != 64
        || !file
            .sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(CoreError::InvalidInput("installed model file"));
    }
    let candidate = root.join(file.relative_path.split('/').collect::<PathBuf>());
    let metadata = fs::symlink_metadata(&candidate)?;
    let canonical = candidate.canonicalize()?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || !canonical.starts_with(root)
        || metadata.len() != file.size_bytes
    {
        return Err(CoreError::ArtifactIntegrity);
    }
    let mut reader = BufReader::new(fs::File::open(canonical)?);
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let sha256 = format!("{:x}", hasher.finalize());
    if sha256 != file.sha256 {
        return Err(CoreError::ArtifactIntegrity);
    }
    Ok(())
}

fn is_safe_model_relative_path(value: &str) -> bool {
    let path = Path::new(value);
    !value.is_empty()
        && value.len() <= 240
        && !value.contains(['\\', ':'])
        && !path.is_absolute()
        && path.components().all(|component| match component {
            Component::Normal(segment) => segment != OsStr::new(".") && segment != OsStr::new(".."),
            Component::CurDir
            | Component::ParentDir
            | Component::RootDir
            | Component::Prefix(_) => false,
        })
}
