use std::{collections::BTreeMap, env, path::PathBuf, sync::Arc, time::Duration};

use tauri::{AppHandle, Manager};

use crate::{
    domain::CoreError,
    hardening::{PrivacyService, RuntimeSessionGuard},
    infrastructure::{
        ArtifactRegistry, AsrPipelineService, AudioPipelineService, ComposerAssetService,
        ComposerExportService, ComposerPipelineService, ModelManager, ProjectLayout,
        ProjectService, TranscriptService, TranslationPipelineService, TtsPipelineService,
    },
    jobs::{InvalidationEngine, PersistentQueue},
    media::{
        FfmpegAdapter, FfprobeAdapter, MediaImportLimits, MediaImportService, MediaToolService,
    },
    persistence::{
        ArtifactRepository, AudioRepository, ComposerRepository, Database, ModelConsentRepository,
        ProjectRepository, SegmentRepository, TranslationRepository, TtsRepository,
    },
    processes::{ApprovedTool, ProcessLimits, SupervisedProcess},
    security::CredentialStore,
    workers::WorkerManager,
};

pub struct AppState {
    pub projects: ProjectService,
    pub queue: PersistentQueue,
    pub media_import: MediaImportService,
    pub media_tools: Option<MediaToolService>,
    pub artifacts: ArtifactRegistry,
    pub transcript: TranscriptService,
    pub model_consents: ModelConsentRepository,
    pub asr_pipeline: AsrPipelineService,
    pub asr_model_path: Option<PathBuf>,
    pub translation_model_path: Option<PathBuf>,
    pub tts_model_path: Option<PathBuf>,
    pub translations: TranslationRepository,
    pub translation_pipeline: TranslationPipelineService,
    pub invalidation: InvalidationEngine,
    pub tts: TtsRepository,
    pub tts_pipeline: TtsPipelineService,
    pub audio: AudioRepository,
    pub audio_pipeline: AudioPipelineService,
    pub composer: ComposerRepository,
    pub composer_export: ComposerExportService,
    pub composer_assets: ComposerAssetService,
    pub composer_pipeline: Option<ComposerPipelineService>,
    pub model_manager: ModelManager,
    pub credentials: Arc<dyn CredentialStore>,
    pub privacy: PrivacyService,
    pub runtime_session: RuntimeSessionGuard,
}

impl AppState {
    pub fn initialize(app: &AppHandle) -> Result<Self, CoreError> {
        let app_data = app
            .path()
            .app_data_dir()
            .map_err(|_| CoreError::Io(std::io::Error::other("app data unavailable")))?;
        let database = Database::open(&app_data.join("vietdub-studio.sqlite3"))?;
        let layout = ProjectLayout::new(app_data.join("projects"))?;
        let runtime_session = RuntimeSessionGuard::begin(database.clone(), &layout)?;
        let privacy = PrivacyService::new(database.clone(), app_data.join("logs"))?;
        let projects =
            ProjectService::new(ProjectRepository::new(database.clone()), layout.clone());
        let artifacts =
            ArtifactRegistry::new(ArtifactRepository::new(database.clone()), layout.clone());
        let media_import = MediaImportService::new(
            projects.clone(),
            artifacts.clone(),
            layout.clone(),
            MediaImportLimits::new(100 * 1024 * 1024 * 1024)?,
        );
        let media_tools = load_media_tools(layout.clone(), artifacts.clone())?;
        let composer_pipeline = load_composer_pipeline(
            layout.clone(),
            artifacts.clone(),
            SegmentRepository::new(database.clone()),
        )?;
        let queue = PersistentQueue::new(database.clone(), 2)?;
        queue.recover_interrupted()?;
        let invalidation = InvalidationEngine::new(database.clone());
        let transcript = TranscriptService::new(
            SegmentRepository::new(database.clone()),
            invalidation.clone(),
        );
        let model_consents = ModelConsentRepository::new(database.clone());
        let translations = TranslationRepository::new(database.clone());
        let tts = TtsRepository::new(database.clone());
        let audio = AudioRepository::new(database.clone());
        let composer = ComposerRepository::new(database.clone());
        let resource_dir = app
            .path()
            .resource_dir()
            .map_err(|_| CoreError::Io(std::io::Error::other("resource directory unavailable")))?;
        let model_manager = ModelManager::load(
            database.clone(),
            &resource_dir.join("manifests").join("models"),
        )?;
        let asr_model_path = load_asr_model(&model_manager, &model_consents)?;
        let translation_model_path = load_translation_model(&model_manager)?;
        let tts_model_path = load_tts_model(&model_manager)?;
        let python_path = env::var_os("VIETDUB_PYTHON_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|| resource_dir.join("python").join("python.exe"));
        let workers_root = env::var_os("VIETDUB_WORKERS_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|| resource_dir.join("workers"));
        let worker_manager = WorkerManager::new(python_path, workers_root, model_consents.clone())
            .with_model_manager(model_manager.clone());
        let tts_python_path = env::var_os("VIETDUB_TTS_PYTHON_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|| resource_dir.join("python").join("python.exe"));
        let mut tts_environment = BTreeMap::new();
        if let Some(cache_root) = env::var_os("VIETDUB_TTS_CACHE_PATH").map(PathBuf::from) {
            let metadata = std::fs::symlink_metadata(&cache_root)?;
            if metadata.file_type().is_symlink() || !metadata.is_dir() || !cache_root.is_absolute()
            {
                return Err(CoreError::UnsafePath);
            }
            let cache_root = cache_root.canonicalize()?;
            tts_environment.insert("HF_HOME".into(), cache_root.clone().into_os_string());
            tts_environment.insert(
                "TRANSFORMERS_CACHE".into(),
                cache_root.join("transformers").into_os_string(),
            );
            tts_environment.insert("TRANSFORMERS_OFFLINE".into(), "1".into());
            tts_environment.insert("HF_HUB_OFFLINE".into(), "1".into());
            tts_environment.insert("NLTK_DATA".into(), cache_root.join("nltk").into_os_string());
        }
        let tts_worker_manager = WorkerManager::new(
            tts_python_path,
            env::var_os("VIETDUB_WORKERS_PATH")
                .map(PathBuf::from)
                .unwrap_or_else(|| resource_dir.join("workers")),
            model_consents.clone(),
        )
        .with_model_manager(model_manager.clone())
        .with_environment(tts_environment);
        let asr_pipeline = AsrPipelineService::new(
            artifacts.clone(),
            layout.clone(),
            transcript.clone(),
            worker_manager.clone(),
        );
        #[cfg(windows)]
        let credentials: Arc<dyn CredentialStore> =
            Arc::new(crate::security::WindowsCredentialStore);
        #[cfg(not(windows))]
        let credentials: Arc<dyn CredentialStore> =
            Arc::new(crate::security::UnavailableCredentialStore);
        let translation_pipeline = TranslationPipelineService::new(
            artifacts.clone(),
            layout.clone(),
            transcript.clone(),
            translations.clone(),
            worker_manager.clone(),
            credentials.clone(),
        );
        let tts_pipeline = TtsPipelineService::new(
            artifacts.clone(),
            layout.clone(),
            transcript.clone(),
            tts.clone(),
            tts_worker_manager,
            credentials.clone(),
        );
        let audio_pipeline = AudioPipelineService::new(
            artifacts.clone(),
            layout.clone(),
            transcript.clone(),
            worker_manager,
        );
        let composer_export = ComposerExportService::new(layout.clone(), artifacts.clone());
        let composer_assets =
            ComposerAssetService::new(layout, artifacts.clone(), 32 * 1024 * 1024)?;
        privacy.write_event(
            "APPLICATION_STARTED",
            &BTreeMap::from([
                (
                    "interruptedSessions".into(),
                    runtime_session.summary().interrupted_sessions.to_string(),
                ),
                (
                    "partialFilesRemoved".into(),
                    runtime_session.summary().partial_files_removed.to_string(),
                ),
            ]),
        )?;
        Ok(Self {
            projects,
            queue,
            media_import,
            media_tools,
            artifacts,
            transcript,
            model_consents,
            asr_pipeline,
            asr_model_path,
            translation_model_path,
            tts_model_path,
            translations,
            translation_pipeline,
            invalidation,
            tts,
            tts_pipeline,
            audio,
            audio_pipeline,
            composer,
            composer_export,
            composer_assets,
            composer_pipeline,
            model_manager,
            credentials,
            privacy,
            runtime_session,
        })
    }
}

fn load_tts_model(manager: &ModelManager) -> Result<Option<PathBuf>, CoreError> {
    const MODEL_ID: &str = "melotts:vi-infore";
    let Some(root) = env::var_os("VIETDUB_TTS_MODEL_PATH").map(PathBuf::from) else {
        return Ok(None);
    };
    let metadata = std::fs::symlink_metadata(&root)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(CoreError::UnsafePath);
    }
    manager.get(MODEL_ID)?;
    Ok(Some(root.canonicalize()?))
}

fn load_translation_model(manager: &ModelManager) -> Result<Option<PathBuf>, CoreError> {
    const MODEL_ID: &str = "opus-mt:zh-vi";
    let Some(root) = env::var_os("VIETDUB_TRANSLATION_MODEL_PATH").map(PathBuf::from) else {
        return Ok(None);
    };
    let metadata = std::fs::symlink_metadata(&root)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(CoreError::UnsafePath);
    }
    manager.get(MODEL_ID)?;
    Ok(Some(root.canonicalize()?))
}

fn load_asr_model(
    manager: &ModelManager,
    consents: &ModelConsentRepository,
) -> Result<Option<PathBuf>, CoreError> {
    const MODEL_ID: &str = "faster-whisper:large-v3";
    let Some(root) = env::var_os("VIETDUB_ASR_MODEL_PATH").map(PathBuf::from) else {
        return Ok(None);
    };
    let metadata = std::fs::symlink_metadata(&root)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(CoreError::UnsafePath);
    }
    if !consents.has_consent(MODEL_ID)? {
        if env::var("VIETDUB_ASR_MODEL_CONSENT").as_deref() != Ok("1") {
            return Err(CoreError::ModelNotConsented);
        }
        let manifest = manager.get(MODEL_ID)?.consent_manifest();
        consents.insert_consent(&manifest)?;
    }
    Ok(Some(root.canonicalize()?))
}

fn load_media_tools(
    layout: ProjectLayout,
    artifacts: ArtifactRegistry,
) -> Result<Option<MediaToolService>, CoreError> {
    let ffmpeg_path = env::var_os("VIETDUB_FFMPEG_PATH").map(PathBuf::from);
    let ffmpeg_sha256 = env::var("VIETDUB_FFMPEG_SHA256").ok();
    let ffprobe_path = env::var_os("VIETDUB_FFPROBE_PATH").map(PathBuf::from);
    let ffprobe_sha256 = env::var("VIETDUB_FFPROBE_SHA256").ok();
    if ffmpeg_path.is_none()
        && ffmpeg_sha256.is_none()
        && ffprobe_path.is_none()
        && ffprobe_sha256.is_none()
    {
        return Ok(None);
    }
    let (Some(ffmpeg_path), Some(ffmpeg_sha256), Some(ffprobe_path), Some(ffprobe_sha256)) =
        (ffmpeg_path, ffmpeg_sha256, ffprobe_path, ffprobe_sha256)
    else {
        return Err(CoreError::InvalidInput("media tool configuration"));
    };
    let ffmpeg =
        ApprovedTool::new(ffmpeg_path, ffmpeg_sha256).map_err(|_| CoreError::ArtifactIntegrity)?;
    let ffprobe = ApprovedTool::new(ffprobe_path, ffprobe_sha256)
        .map_err(|_| CoreError::ArtifactIntegrity)?;
    let probe_supervisor = SupervisedProcess::new(ProcessLimits {
        timeout: Duration::from_secs(30),
        max_stdout_bytes: 4 * 1024 * 1024,
        max_stderr_bytes: 128 * 1024,
    })
    .map_err(|_| CoreError::InvalidInput("ffprobe process limits"))?;
    let ffmpeg_supervisor = SupervisedProcess::new(ProcessLimits {
        timeout: Duration::from_secs(6 * 60 * 60),
        max_stdout_bytes: 512 * 1024,
        max_stderr_bytes: 512 * 1024,
    })
    .map_err(|_| CoreError::InvalidInput("ffmpeg process limits"))?;
    Ok(Some(MediaToolService::new(
        layout,
        artifacts,
        FfprobeAdapter::new(ffprobe, probe_supervisor),
        FfmpegAdapter::new(ffmpeg, ffmpeg_supervisor),
    )))
}

fn load_composer_pipeline(
    layout: ProjectLayout,
    artifacts: ArtifactRegistry,
    segments: SegmentRepository,
) -> Result<Option<ComposerPipelineService>, CoreError> {
    let ffmpeg_path = env::var_os("VIETDUB_FFMPEG_PATH").map(PathBuf::from);
    let ffmpeg_sha256 = env::var("VIETDUB_FFMPEG_SHA256").ok();
    let ffprobe_path = env::var_os("VIETDUB_FFPROBE_PATH").map(PathBuf::from);
    let ffprobe_sha256 = env::var("VIETDUB_FFPROBE_SHA256").ok();
    if ffmpeg_path.is_none()
        && ffmpeg_sha256.is_none()
        && ffprobe_path.is_none()
        && ffprobe_sha256.is_none()
    {
        return Ok(None);
    }
    let (Some(ffmpeg_path), Some(ffmpeg_sha256), Some(ffprobe_path), Some(ffprobe_sha256)) =
        (ffmpeg_path, ffmpeg_sha256, ffprobe_path, ffprobe_sha256)
    else {
        return Err(CoreError::InvalidInput("media tool configuration"));
    };
    let ffmpeg =
        ApprovedTool::new(ffmpeg_path, ffmpeg_sha256).map_err(|_| CoreError::ArtifactIntegrity)?;
    let ffprobe = ApprovedTool::new(ffprobe_path, ffprobe_sha256)
        .map_err(|_| CoreError::ArtifactIntegrity)?;
    let probe_supervisor = SupervisedProcess::new(ProcessLimits {
        timeout: Duration::from_secs(30),
        max_stdout_bytes: 4 * 1024 * 1024,
        max_stderr_bytes: 128 * 1024,
    })
    .map_err(|_| CoreError::InvalidInput("ffprobe process limits"))?;
    let render_supervisor = SupervisedProcess::new(ProcessLimits {
        timeout: Duration::from_secs(12 * 60 * 60),
        max_stdout_bytes: 512 * 1024,
        max_stderr_bytes: 1024 * 1024,
    })
    .map_err(|_| CoreError::InvalidInput("ffmpeg process limits"))?;
    Ok(Some(ComposerPipelineService::new(
        layout,
        artifacts,
        segments,
        ffmpeg,
        FfprobeAdapter::new(ffprobe, probe_supervisor),
        render_supervisor,
    )))
}
