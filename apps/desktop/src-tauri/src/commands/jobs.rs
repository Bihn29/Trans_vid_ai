use std::collections::BTreeMap;

use serde_json::Map;
use sha2::{Digest, Sha256};
use tauri::State;
use uuid::Uuid;

use crate::{
    domain::{
        ArtifactKind, ArtifactVerification, BlurRegion, CacheDescriptor, ComposerConfig, CoreError,
        Job, JobStatus, MediaMetadata, NewStageRun, PreviewPreset, SeparationEngineDescriptor,
        StageName, StageScope, SubtitleMode, TimedRegion, TranslationProviderDisclosure,
        VoiceScope,
    },
    infrastructure::{
        AsrExecutionRequest, AudioMixRequest, ComposerExecutionRequest, LocalTtsExecutionRequest,
        SeparationExecutionRequest, TranslationExecutionRequest,
    },
    media::MediaToolError,
    state::AppState,
};

use super::projects::CommandError;

#[tauri::command]
pub fn start_transcript_job(
    state: State<'_, AppState>,
    project_id: Uuid,
) -> Result<Job, CommandError> {
    let project_jobs = state.queue.list_for_project(project_id)?;
    if project_jobs.iter().any(|job| {
        matches!(
            job.status,
            JobStatus::Queued | JobStatus::Running | JobStatus::Paused
        )
    }) {
        return Err(CoreError::Conflict("active project job").into());
    }
    let project = state.projects.get(project_id)?;
    let source_artifact_id = project
        .source_asset_id
        .ok_or(CoreError::NotFound("source artifact"))?;
    let source = state.artifacts.get(source_artifact_id)?;
    if state.artifacts.verify(source.id)? != ArtifactVerification::Verified
        || project
            .config_snapshot
            .get("source")
            .and_then(|value| value.get("import_status"))
            .and_then(|value| value.as_str())
            != Some("ready")
    {
        return Err(CoreError::InvalidTransition.into());
    }
    let existing_segments = state.transcript.get_transcript(project_id)?;
    if !existing_segments.is_empty() {
        if existing_segments
            .iter()
            .all(|segment| !segment.translated_text.trim().is_empty())
        {
            return project_jobs
                .into_iter()
                .rev()
                .find(|job| {
                    job.job_type == StageName::Translate && job.status == JobStatus::Completed
                })
                .ok_or(CoreError::InvalidTransition.into());
        }
        return enqueue_local_translation(
            &state.queue,
            &state.transcript,
            state.translation_pipeline.clone(),
            state.translation_model_path.clone(),
            state.privacy.clone(),
            project_id,
        )
        .map_err(Into::into);
    }
    let media_tools = state
        .media_tools
        .clone()
        .ok_or(CoreError::MediaToolsUnavailable)?;
    let stage = pipeline_stage(
        project_id,
        StageName::ExtractAudio,
        &source.sha256,
        b"extract-audio-v1",
        "ffmpeg",
        "none",
    );
    let job = state.queue.enqueue(&stage, 100)?;
    state.privacy.write_event(
        "TRANSCRIPT_JOB_CREATED",
        &BTreeMap::from([
            ("projectId".into(), project_id.to_string()),
            ("jobId".into(), job.id.to_string()),
            ("stage".into(), StageName::ExtractAudio.as_str().into()),
        ]),
    )?;

    let queue = state.queue.clone();
    let privacy = state.privacy.clone();
    let asr_pipeline = state.asr_pipeline.clone();
    let asr_model_path = state.asr_model_path.clone();
    let transcript = state.transcript.clone();
    let translation_pipeline = state.translation_pipeline.clone();
    let translation_model_path = state.translation_model_path.clone();
    let job_id = job.id;
    tauri::async_runtime::spawn(async move {
        let claimed = match queue.claim(job_id) {
            Ok(claimed) => claimed,
            Err(_) => return,
        };
        let _ = queue.update_progress(job_id, 5.0);
        let _ = privacy.write_event(
            "WORKER_SPAWNED",
            &BTreeMap::from([
                ("projectId".into(), project_id.to_string()),
                ("jobId".into(), job_id.to_string()),
                ("stage".into(), StageName::ExtractAudio.as_str().into()),
                ("engine".into(), "ffmpeg".into()),
            ]),
        );
        let _ = privacy.write_event(
            "WORKER_PROGRESS",
            &BTreeMap::from([
                ("projectId".into(), project_id.to_string()),
                ("jobId".into(), job_id.to_string()),
                ("progress".into(), "5".into()),
            ]),
        );
        let extraction = media_tools
            .extract_normalized_audio(project_id, source_artifact_id, claimed.cancellation)
            .await;
        let audio = match extraction {
            Ok(audio) => audio,
            Err(error) => {
                let code = media_tool_error_code(&error);
                let _ = queue.fail(job_id, code, "Không thể tách âm thanh từ video nguồn.");
                let _ = privacy.write_event(
                    "WORKER_FAILED",
                    &BTreeMap::from([
                        ("projectId".into(), project_id.to_string()),
                        ("jobId".into(), job_id.to_string()),
                        ("stage".into(), StageName::ExtractAudio.as_str().into()),
                        ("errorCode".into(), code.into()),
                    ]),
                );
                return;
            }
        };
        let _ = queue.update_progress(job_id, 95.0);
        let _ = privacy.write_event(
            "WORKER_PROGRESS",
            &BTreeMap::from([
                ("projectId".into(), project_id.to_string()),
                ("jobId".into(), job_id.to_string()),
                ("progress".into(), "95".into()),
            ]),
        );
        if queue.complete(job_id, &[audio.id]).is_err() {
            return;
        }
        let _ = privacy.write_event(
            "WORKER_COMPLETED",
            &BTreeMap::from([
                ("projectId".into(), project_id.to_string()),
                ("jobId".into(), job_id.to_string()),
                ("stage".into(), StageName::ExtractAudio.as_str().into()),
                ("progress".into(), "100".into()),
            ]),
        );

        let transcribe_stage = pipeline_stage(
            project_id,
            StageName::Transcribe,
            &audio.sha256,
            b"transcribe-zh-v1",
            "local-asr",
            "unconfigured",
        );
        let transcribe_job = match queue.enqueue(&transcribe_stage, 100) {
            Ok(job) => job,
            Err(_) => return,
        };
        let transcribe_claimed = match queue.claim(transcribe_job.id) {
            Ok(claimed) => claimed,
            Err(_) => return,
        };
        let Some(model_path) = asr_model_path else {
            let _ = queue.update_progress(transcribe_claimed.job.id, 1.0);
            let _ = queue.fail(
                transcribe_claimed.job.id,
                "ASR_MODEL_UNAVAILABLE",
                "Chưa có mô hình nhận dạng giọng nói đã được xác minh. Hãy cài Faster Whisper để tiếp tục.",
            );
            let _ = privacy.write_event(
                "TRANSCRIPT_JOB_FAILED",
                &BTreeMap::from([
                    ("projectId".into(), project_id.to_string()),
                    ("jobId".into(), transcribe_claimed.job.id.to_string()),
                    ("errorCode".into(), "ASR_MODEL_UNAVAILABLE".into()),
                ]),
            );
            return;
        };
        let transcribe_job_id = transcribe_claimed.job.id;
        let _ = queue.update_progress(transcribe_job_id, 5.0);
        let _ = privacy.write_event(
            "WORKER_SPAWNED",
            &BTreeMap::from([
                ("projectId".into(), project_id.to_string()),
                ("jobId".into(), transcribe_job_id.to_string()),
                ("stage".into(), StageName::Transcribe.as_str().into()),
                ("engine".into(), "faster-whisper".into()),
            ]),
        );
        let execution = AsrExecutionRequest {
            audio_artifact_id: audio.id,
            primary_model_id: "faster-whisper:large-v3".into(),
            primary_model_path: model_path,
            fallback_model_id: None,
            fallback_model_path: None,
            language: "zh".into(),
            region: None,
        };
        match asr_pipeline
            .execute_claimed(&queue, transcribe_claimed, &execution)
            .await
        {
            Ok(result) => {
                let _ = privacy.write_event(
                    "WORKER_COMPLETED",
                    &BTreeMap::from([
                        ("projectId".into(), project_id.to_string()),
                        ("jobId".into(), transcribe_job_id.to_string()),
                        ("stage".into(), StageName::Transcribe.as_str().into()),
                        ("segments".into(), result.segments.len().to_string()),
                    ]),
                );
                let _ = enqueue_local_translation(
                    &queue,
                    &transcript,
                    translation_pipeline.clone(),
                    translation_model_path.clone(),
                    privacy.clone(),
                    project_id,
                );
            }
            Err(error) => {
                let _ = privacy.write_event(
                    "WORKER_FAILED",
                    &BTreeMap::from([
                        ("projectId".into(), project_id.to_string()),
                        ("jobId".into(), transcribe_job_id.to_string()),
                        ("stage".into(), StageName::Transcribe.as_str().into()),
                        ("errorCode".into(), error.code().into()),
                    ]),
                );
            }
        }
    });
    Ok(job)
}

#[tauri::command]
pub fn start_dub_render_job(
    state: State<'_, AppState>,
    project_id: Uuid,
) -> Result<Job, CommandError> {
    if state.queue.list_for_project(project_id)?.iter().any(|job| {
        matches!(
            job.status,
            JobStatus::Queued | JobStatus::Running | JobStatus::Paused
        )
    }) {
        return Err(CoreError::Conflict("active project job").into());
    }
    let model_path = state
        .tts_model_path
        .clone()
        .ok_or(CoreError::NotFound("local TTS model"))?;
    let composer_pipeline = state
        .composer_pipeline
        .clone()
        .ok_or(CoreError::MediaToolsUnavailable)?;
    let project = state.projects.get(project_id)?;
    let source_artifact_id = project
        .source_asset_id
        .ok_or(CoreError::NotFound("source artifact"))?;
    let source = state.artifacts.get(source_artifact_id)?;
    let segments = state.transcript.get_transcript(project_id)?;
    if segments.is_empty()
        || segments
            .iter()
            .any(|segment| segment.enabled && segment.translated_text.trim().is_empty())
    {
        return Err(CoreError::InvalidTransition.into());
    }
    let source_metadata: crate::domain::MediaMetadata = serde_json::from_value(
        project
            .config_snapshot
            .get("source")
            .and_then(|value| value.get("metadata"))
            .cloned()
            .ok_or(CoreError::InvalidInput("source metadata"))?,
    )
    .map_err(|_| CoreError::InvalidInput("source metadata"))?;
    let original_audio = state
        .artifacts
        .list_for_project(project_id)?
        .into_iter()
        .rev()
        .find(|artifact| artifact.kind == ArtifactKind::OriginalAudio)
        .ok_or(CoreError::NotFound("original audio"))?;

    // A failed downstream mix/render must not make the user synthesize a long
    // project again. Reuse verified per-segment voices and the latest verified
    // background track when they are still attached to the current transcript.
    let reusable_tts = segments
        .iter()
        .filter(|segment| segment.enabled)
        .map(|segment| {
            let id = segment.audio_artifact_id?;
            let artifact = state.artifacts.get(id).ok()?;
            (artifact.kind == ArtifactKind::Tts
                && state.artifacts.verify(id).ok() == Some(ArtifactVerification::Verified))
            .then_some(artifact)
        })
        .collect::<Option<Vec<_>>>();
    let reusable_background = state
        .artifacts
        .list_for_project(project_id)?
        .into_iter()
        .rev()
        .find(|artifact| {
            artifact.kind == ArtifactKind::Background
                && state.artifacts.verify(artifact.id).ok() == Some(ArtifactVerification::Verified)
        });
    if let (Some(tts_artifacts), Some(background)) = (reusable_tts, reusable_background) {
        let settings = state.audio.get_settings(project_id)?;
        let mut mix_hasher = Sha256::new();
        mix_hasher.update(background.sha256.as_bytes());
        for artifact in &tts_artifacts {
            mix_hasher.update(artifact.sha256.as_bytes());
        }
        let mix_stage = pipeline_stage(
            project_id,
            StageName::MixAudio,
            &format!("{:x}", mix_hasher.finalize()),
            &serde_json::to_vec(&settings).unwrap_or_default(),
            "typed-rust-mixer",
            "none",
        );
        let mix_job = state.queue.enqueue(&mix_stage, 100)?;
        let queue = state.queue.clone();
        let audio_pipeline = state.audio_pipeline.clone();
        let composer_pipeline = composer_pipeline.clone();
        let source_hash = source.sha256.clone();
        let mix_job_id = mix_job.id;
        tauri::async_runtime::spawn(async move {
            let claimed = match queue.claim(mix_job_id) {
                Ok(value) => value,
                Err(_) => return,
            };
            let mixed = match audio_pipeline.execute_mix_claimed(
                &queue,
                claimed,
                &AudioMixRequest {
                    background_artifact_id: background.id,
                    original_voice_artifact_id: None,
                    music_artifact_id: None,
                    settings,
                },
            ) {
                Ok(value) => value,
                Err(_) => return,
            };
            let config = dub_composer_config(project_id, &source_metadata);
            let render_input_hash = format!(
                "{:x}",
                Sha256::digest(format!("{}{}", source_hash, mixed.artifact.sha256).as_bytes())
            );
            let render_stage = pipeline_stage(
                project_id,
                StageName::Render,
                &render_input_hash,
                &serde_json::to_vec(&config).unwrap_or_default(),
                "ffmpeg-composer",
                "none",
            );
            let render_job = match queue.enqueue(&render_stage, 100) {
                Ok(value) => value,
                Err(_) => return,
            };
            let render_claimed = match queue.claim(render_job.id) {
                Ok(value) => value,
                Err(_) => return,
            };
            let _ = composer_pipeline
                .execute_claimed(
                    &queue,
                    render_claimed,
                    &ComposerExecutionRequest {
                        project_id,
                        source_artifact_id,
                        mixed_audio_artifact_id: mixed.artifact.id,
                        config,
                    },
                )
                .await;
        });
        return Ok(mix_job);
    }
    state
        .tts
        .set_assignment(project_id, VoiceScope::Project, "local-melo", "vi-default")?;

    let mut tts_input = Sha256::new();
    for segment in &segments {
        if segment.enabled {
            tts_input.update(segment.id.as_bytes());
            tts_input.update(segment.translated_text.as_bytes());
        }
    }
    let tts_input_hash = format!("{:x}", tts_input.finalize());
    let tts_stage = pipeline_stage(
        project_id,
        StageName::Synthesize,
        &tts_input_hash,
        b"melotts-vi-infore-v1",
        "local-melo",
        "melotts:vi-infore",
    );
    let job = state.queue.enqueue(&tts_stage, 100)?;
    let queue = state.queue.clone();
    let tts_pipeline = state.tts_pipeline.clone();
    let audio_pipeline = state.audio_pipeline.clone();
    let settings = state.audio.get_settings(project_id)?;
    let privacy = state.privacy.clone();
    let source_hash = source.sha256.clone();
    let original_hash = original_audio.sha256.clone();
    let job_id = job.id;
    tauri::async_runtime::spawn(async move {
        let claimed = match queue.claim(job_id) {
            Ok(value) => value,
            Err(_) => return,
        };
        let tts_result = tts_pipeline
            .execute_local_claimed(
                &queue,
                claimed,
                &LocalTtsExecutionRequest {
                    model_id: "melotts:vi-infore".into(),
                    model_path,
                    speed: 1.0,
                },
            )
            .await;
        let tts_result = match tts_result {
            Ok(value) => value,
            Err(error) => {
                let _ = privacy.write_event(
                    "WORKER_FAILED",
                    &BTreeMap::from([
                        ("projectId".into(), project_id.to_string()),
                        ("jobId".into(), job_id.to_string()),
                        ("stage".into(), StageName::Synthesize.as_str().into()),
                        ("errorCode".into(), error.code().into()),
                    ]),
                );
                return;
            }
        };

        let separation_stage = pipeline_stage(
            project_id,
            StageName::SeparateAudio,
            &original_hash,
            b"energy-mask-v1-default",
            "energy-mask-v1",
            "none",
        );
        let separation_job = match queue.enqueue(&separation_stage, 100) {
            Ok(value) => value,
            Err(_) => return,
        };
        let separation_claimed = match queue.claim(separation_job.id) {
            Ok(value) => value,
            Err(_) => return,
        };
        let separation = audio_pipeline
            .execute_separation_claimed(
                &queue,
                separation_claimed,
                &SeparationExecutionRequest {
                    source_artifact_id: original_audio.id,
                    engine: SeparationEngineDescriptor {
                        engine_id: "energy-mask-v1".into(),
                        display_name: "VietDub Energy Mask".into(),
                        version: "1.0.0".into(),
                        license: "UNLICENSED".into(),
                        install_mode: "bundled_source".into(),
                        requires_consent: false,
                        sends_data_off_device: false,
                        approved: true,
                    },
                    energy_threshold: 0.12,
                },
            )
            .await;
        let separation = match separation {
            Ok(value) => value,
            Err(_) => return,
        };

        let mut mix_hasher = Sha256::new();
        mix_hasher.update(separation.background.sha256.as_bytes());
        for artifact in &tts_result.artifacts {
            mix_hasher.update(artifact.sha256.as_bytes());
        }
        let mix_stage = pipeline_stage(
            project_id,
            StageName::MixAudio,
            &format!("{:x}", mix_hasher.finalize()),
            &serde_json::to_vec(&settings).unwrap_or_default(),
            "typed-rust-mixer",
            "none",
        );
        let mix_job = match queue.enqueue(&mix_stage, 100) {
            Ok(value) => value,
            Err(_) => return,
        };
        let mix_claimed = match queue.claim(mix_job.id) {
            Ok(value) => value,
            Err(_) => return,
        };
        let mixed = match audio_pipeline.execute_mix_claimed(
            &queue,
            mix_claimed,
            &AudioMixRequest {
                background_artifact_id: separation.background.id,
                original_voice_artifact_id: None,
                music_artifact_id: None,
                settings,
            },
        ) {
            Ok(value) => value,
            Err(_) => return,
        };

        let config = dub_composer_config(project_id, &source_metadata);
        let render_input_hash = format!(
            "{:x}",
            Sha256::digest(format!("{}{}", source_hash, mixed.artifact.sha256).as_bytes())
        );
        let render_stage = pipeline_stage(
            project_id,
            StageName::Render,
            &render_input_hash,
            &serde_json::to_vec(&config).unwrap_or_default(),
            "ffmpeg-composer",
            "none",
        );
        let render_job = match queue.enqueue(&render_stage, 100) {
            Ok(value) => value,
            Err(_) => return,
        };
        let render_claimed = match queue.claim(render_job.id) {
            Ok(value) => value,
            Err(_) => return,
        };
        let _ = composer_pipeline
            .execute_claimed(
                &queue,
                render_claimed,
                &ComposerExecutionRequest {
                    project_id,
                    source_artifact_id,
                    mixed_audio_artifact_id: mixed.artifact.id,
                    config,
                },
            )
            .await;
    });
    Ok(job)
}

fn dub_composer_config(project_id: Uuid, source: &MediaMetadata) -> ComposerConfig {
    let mut config = ComposerConfig::defaults(project_id);
    config.subtitle_mode = SubtitleMode::Burned;
    config.preview_preset = PreviewPreset::Draft;
    config.blur_regions.push(BlurRegion {
        region: TimedRegion {
            x: source.width / 10,
            y: source.height * 84 / 100,
            width: source.width * 8 / 10,
            height: source.height * 15 / 100,
            start_ms: 0,
            end_ms: source.duration_ms,
            opacity: 1.0,
        },
        radius: 12.0,
    });
    config
}

fn enqueue_local_translation(
    queue: &crate::jobs::PersistentQueue,
    transcript: &crate::infrastructure::TranscriptService,
    translation_pipeline: crate::infrastructure::TranslationPipelineService,
    model_path: Option<std::path::PathBuf>,
    privacy: crate::hardening::PrivacyService,
    project_id: Uuid,
) -> Result<Job, CoreError> {
    let model_path = model_path.ok_or(CoreError::TranslationModelUnavailable)?;
    let segments = transcript.approve_transcript(project_id)?;
    let _ = queue.complete_review(project_id);
    let mut input_hasher = Sha256::new();
    for segment in &segments {
        input_hasher.update(segment.id.as_bytes());
        input_hasher.update(segment.start_ms.to_le_bytes());
        input_hasher.update(segment.end_ms.to_le_bytes());
        input_hasher.update(segment.source_text.as_bytes());
    }
    let input_hash = format!("{:x}", input_hasher.finalize());
    let stage = pipeline_stage(
        project_id,
        StageName::Translate,
        &input_hash,
        b"translate-zh-vi-opus-v1",
        "local-translation",
        "opus-mt:zh-vi",
    );
    let job = queue.enqueue(&stage, 100)?;
    let job_id = job.id;
    let queue = queue.clone();
    tauri::async_runtime::spawn(async move {
        let claimed = match queue.claim(job_id) {
            Ok(claimed) => claimed,
            Err(_) => return,
        };
        let _ = queue.update_progress(job_id, 1.0);
        let _ = privacy.write_event(
            "WORKER_SPAWNED",
            &BTreeMap::from([
                ("projectId".into(), project_id.to_string()),
                ("jobId".into(), job_id.to_string()),
                ("stage".into(), StageName::Translate.as_str().into()),
                ("engine".into(), "opus-mt-zh-vi".into()),
            ]),
        );
        let execution = TranslationExecutionRequest {
            disclosure: TranslationProviderDisclosure {
                provider_id: "local".into(),
                display_name: "OPUS-MT Trung–Việt".into(),
                sends_data_off_device: false,
            },
            endpoint: None,
            model: "opus-mt:zh-vi".into(),
            local_model_path: Some(model_path),
            credential: None,
            cloud_consent: false,
            source_language: "zh".into(),
            target_language: "vi".into(),
            block_size: 20,
            max_attempts: 1,
        };
        match translation_pipeline
            .execute_claimed(&queue, claimed, &execution)
            .await
        {
            Ok(result) => {
                let _ = privacy.write_event(
                    "WORKER_COMPLETED",
                    &BTreeMap::from([
                        ("projectId".into(), project_id.to_string()),
                        ("jobId".into(), job_id.to_string()),
                        ("stage".into(), StageName::Translate.as_str().into()),
                        ("blocks".into(), result.blocks.len().to_string()),
                    ]),
                );
            }
            Err(error) => {
                let _ = privacy.write_event(
                    "WORKER_FAILED",
                    &BTreeMap::from([
                        ("projectId".into(), project_id.to_string()),
                        ("jobId".into(), job_id.to_string()),
                        ("stage".into(), StageName::Translate.as_str().into()),
                        ("errorCode".into(), error.code().into()),
                    ]),
                );
            }
        }
    });
    Ok(job)
}

#[tauri::command]
pub fn list_project_jobs(
    state: State<'_, AppState>,
    project_id: Uuid,
) -> Result<Vec<Job>, CommandError> {
    state.queue.list_for_project(project_id).map_err(Into::into)
}

#[tauri::command]
pub fn cancel_job(state: State<'_, AppState>, job_id: Uuid) -> Result<Job, CommandError> {
    state.queue.cancel(job_id).map_err(Into::into)
}

fn pipeline_stage(
    project_id: Uuid,
    name: StageName,
    input_hash: &str,
    config_identity: &[u8],
    engine_name: &str,
    model_version: &str,
) -> NewStageRun {
    let config_hash = format!("{:x}", Sha256::digest(config_identity));
    let mut stage = NewStageRun::new(
        project_id,
        name,
        StageScope::Project,
        CacheDescriptor {
            schema_version: 1,
            input_hash: input_hash.into(),
            config_hash,
            engine_name: engine_name.into(),
            engine_version: "1".into(),
            model_version: model_version.into(),
            metadata: Map::new(),
        },
        engine_name,
    );
    stage.model_version = model_version.into();
    stage
}

fn media_tool_error_code(error: &MediaToolError) -> &'static str {
    match error {
        MediaToolError::InvalidMetadata => "INVALID_MEDIA_METADATA",
        MediaToolError::MissingOutput => "MEDIA_TOOL_MISSING_OUTPUT",
        MediaToolError::Tool(_) => "FFMPEG_FAILED",
        MediaToolError::Io(_) => "FILESYSTEM_ERROR",
        MediaToolError::Core(error) => error.code(),
    }
}
