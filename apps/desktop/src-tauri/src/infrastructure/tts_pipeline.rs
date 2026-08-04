use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{fs, path::PathBuf, sync::Arc};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::{sha256_file, ArtifactRegistry, ProjectLayout, ProjectRelativePath, TranscriptService};
use crate::{
    domain::{
        duration_fit, Artifact, ArtifactKind, ArtifactVerification, CoreError, NewTtsSegmentRun,
        StageName, TtsRunStatus, TtsSegmentRun, VoiceDescriptor,
    },
    jobs::{ClaimedJob, PersistentQueue},
    persistence::TtsRepository,
    security::{CredentialReference, CredentialStore},
    workers::{RequiredModel, WorkerManager, WorkerRequest},
};

#[derive(Debug, Clone)]
pub struct TtsExecutionRequest {
    pub provider: VoiceDescriptor,
    pub endpoint: String,
    pub model: String,
    pub credential: CredentialReference,
    pub cloud_consent: bool,
    pub speed: f64,
    pub max_attempts: u8,
}

#[derive(Debug, Clone)]
pub struct LocalTtsExecutionRequest {
    pub model_id: String,
    pub model_path: PathBuf,
    pub speed: f64,
}

impl LocalTtsExecutionRequest {
    fn validate(&self) -> Result<(), CoreError> {
        if self.model_id != "melotts:vi-infore" || !(0.5..=2.0).contains(&self.speed) {
            return Err(CoreError::InvalidInput("local TTS execution"));
        }
        let metadata = fs::symlink_metadata(&self.model_path)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(CoreError::UnsafePath);
        }
        Ok(())
    }
}
impl TtsExecutionRequest {
    fn validate(&self) -> Result<(), CoreError> {
        if !self.provider.approved
            || !self.provider.sends_data_off_device
            || self.provider.provider_id != "openai-compatible"
            || !matches!(self.provider.voice_id.as_str(), "alloy" | "nova")
            || !self.cloud_consent
            || self.endpoint.is_empty()
            || self.endpoint.len() > 2_048
            || self.endpoint.chars().any(char::is_control)
            || self.model.is_empty()
            || self.model.len() > 128
            || self.model.chars().any(char::is_control)
            || !(0.25..=4.0).contains(&self.speed)
            || !(1..=3).contains(&self.max_attempts)
        {
            return Err(CoreError::InvalidInput("TTS execution"));
        }
        Ok(())
    }
}
#[derive(Debug, Clone)]
pub struct TtsExecutionResult {
    pub runs: Vec<TtsSegmentRun>,
    pub artifacts: Vec<Artifact>,
    pub warnings: Vec<(Uuid, String)>,
}
#[derive(Clone)]
pub struct TtsPipelineService {
    artifacts: ArtifactRegistry,
    layout: ProjectLayout,
    transcript: TranscriptService,
    tts: TtsRepository,
    workers: WorkerManager,
    credentials: Arc<dyn CredentialStore>,
}
impl TtsPipelineService {
    pub fn new(
        artifacts: ArtifactRegistry,
        layout: ProjectLayout,
        transcript: TranscriptService,
        tts: TtsRepository,
        workers: WorkerManager,
        credentials: Arc<dyn CredentialStore>,
    ) -> Self {
        Self {
            artifacts,
            layout,
            transcript,
            tts,
            workers,
            credentials,
        }
    }

    pub async fn execute_local_claimed(
        &self,
        queue: &PersistentQueue,
        claimed: ClaimedJob,
        execution: &LocalTtsExecutionRequest,
    ) -> Result<TtsExecutionResult, CoreError> {
        if claimed.job.job_type != StageName::Synthesize || execution.validate().is_err() {
            let error = CoreError::InvalidInput("local TTS job");
            queue.fail(
                claimed.job.id,
                error.code(),
                "Cấu hình giọng Việt cục bộ không hợp lệ.",
            )?;
            return Err(error);
        }
        let segments = self
            .transcript
            .get_transcript(claimed.job.project_id)?
            .into_iter()
            .filter(|segment| segment.enabled && !segment.translated_text.trim().is_empty())
            .collect::<Vec<_>>();
        if segments.is_empty() {
            queue.fail(
                claimed.job.id,
                "INVALID_INPUT",
                "Không có bản dịch để tạo giọng đọc.",
            )?;
            return Err(CoreError::InvalidInput("local TTS segments"));
        }
        let definitions = segments
            .iter()
            .map(|segment| {
                Ok(NewTtsSegmentRun {
                    id: Uuid::new_v4(),
                    project_id: segment.project_id,
                    stage_run_id: claimed.job.stage_run_id,
                    segment_id: segment.id,
                    cache_identity: identity(
                        &segment.translation_hash,
                        "local-melo",
                        "vi-default",
                        &execution.model_id,
                        execution.speed,
                    ),
                    provider_id: "local-melo".into(),
                    voice_id: "vi-default".into(),
                    target_duration_ms: segment.end_ms - segment.start_ms,
                })
            })
            .collect::<Result<Vec<_>, CoreError>>()?;
        self.tts.recover_stage(claimed.job.stage_run_id)?;
        let mut runs = self.tts.list_for_stage(claimed.job.stage_run_id)?;
        if runs.is_empty() {
            runs = self.tts.insert_runs(&definitions)?;
        } else if runs.len() != definitions.len()
            || runs.iter().zip(&definitions).any(|(left, right)| {
                left.segment_id != right.segment_id || left.cache_identity != right.cache_identity
            })
        {
            queue.fail(
                claimed.job.id,
                "INVALID_INPUT",
                "Dữ liệu giọng đọc không còn khớp bản dịch.",
            )?;
            return Err(CoreError::InvalidInput("stale local TTS runs"));
        }

        let mut active_runs = Vec::new();
        for run in &mut runs {
            if run.status == TtsRunStatus::Completed
                && run.artifact_id.is_some_and(|id| {
                    self.artifacts.verify(id).ok() == Some(ArtifactVerification::Verified)
                })
            {
                continue;
            }
            if run.status == TtsRunStatus::Completed {
                *run = self.tts.invalidate_cached(run.id)?;
            }
            active_runs.push(self.tts.mark_running(run.id)?);
        }
        if claimed.cancellation.is_cancelled() {
            queue.acknowledge_interruption(claimed.job.id)?;
            return Err(CoreError::WorkerExecution);
        }
        let mut artifacts = Vec::new();
        let mut warnings = Vec::new();
        if !active_runs.is_empty() {
            let root = self.layout.project_root(claimed.job.project_id)?;
            let client = self.workers.client_for_stage(
                StageName::Synthesize,
                &root,
                &[RequiredModel {
                    model_id: execution.model_id.clone(),
                    root: execution.model_path.clone(),
                }],
            )?;
            let mut request =
                WorkerRequest::new("synthesize_batch", claimed.job.project_id, "audio/tts");
            request.input.insert(
                "tts_batch".into(),
                json!({
                    "schema_version": 1,
                    "items": active_runs.iter().map(|run| {
                        let segment = segments.iter().find(|segment| segment.id == run.segment_id)
                            .ok_or(CoreError::NotFound("segment"))?;
                        Ok(json!({
                            "segment_id": segment.id,
                            "text": segment.translated_text,
                            "voice_id": run.voice_id,
                            "speed": execution.speed,
                        }))
                    }).collect::<Result<Vec<_>, CoreError>>()?,
                }),
            );
            request
                .config
                .insert("provider_id".into(), Value::String("local-melo".into()));
            request.config.insert(
                "model_path".into(),
                Value::String(
                    execution
                        .model_path
                        .to_str()
                        .ok_or(CoreError::UnsafePath)?
                        .to_owned(),
                ),
            );
            request
                .config
                .insert("cloud_consent".into(), Value::Bool(false));
            let job_id = claimed.job.id;
            let response = client
                .run_with_progress(&request, claimed.cancellation.clone(), |progress| {
                    let _ = queue.update_progress(job_id, f64::from(progress.progress));
                })
                .await;
            let descriptors = match response {
                Ok(response) => response.artifacts,
                Err(_) => {
                    for run in &active_runs {
                        let _ = self.tts.fail(run.id, "LOCAL_TTS_FAILED");
                    }
                    queue.fail(
                        job_id,
                        "LOCAL_TTS_FAILED",
                        "Không thể tạo giọng đọc tiếng Việt.",
                    )?;
                    return Err(CoreError::WorkerExecution);
                }
            };
            if descriptors.len() != active_runs.len() {
                queue.fail(
                    job_id,
                    "INVALID_TTS_AUDIO",
                    "Số lượng đoạn giọng đọc không hợp lệ.",
                )?;
                return Err(CoreError::InvalidInput("local TTS artifacts"));
            }
            for active in active_runs {
                let descriptor = descriptors
                    .iter()
                    .find(|descriptor| {
                        descriptor
                            .metadata
                            .get("segment_id")
                            .and_then(Value::as_str)
                            == Some(active.segment_id.to_string().as_str())
                    })
                    .ok_or(CoreError::InvalidInput("local TTS segment artifact"))?;
                let (updated, artifact) = self.consume(active, std::slice::from_ref(descriptor))?;
                if let Some(code) = &updated.warning_code {
                    warnings.push((updated.segment_id, code.clone()));
                }
                if let Some(run) = runs.iter_mut().find(|run| run.id == updated.id) {
                    *run = updated;
                }
                artifacts.push(artifact);
            }
        }
        queue.complete(
            claimed.job.id,
            &runs
                .iter()
                .filter_map(|run| run.artifact_id)
                .collect::<Vec<_>>(),
        )?;
        Ok(TtsExecutionResult {
            runs,
            artifacts,
            warnings,
        })
    }
    pub async fn execute_claimed(
        &self,
        queue: &PersistentQueue,
        claimed: ClaimedJob,
        execution: &TtsExecutionRequest,
    ) -> Result<TtsExecutionResult, CoreError> {
        if claimed.job.job_type != StageName::Synthesize {
            return Err(CoreError::InvalidInput("TTS job stage"));
        }
        if let Err(e) = execution.validate() {
            queue.fail(claimed.job.id, e.code(), "Cấu hình TTS không hợp lệ.")?;
            return Err(e);
        }
        let transcript = match self.transcript.get_transcript(claimed.job.project_id) {
            Ok(value) => value,
            Err(error) => {
                queue.fail(claimed.job.id, error.code(), "Không thể đọc bản dịch TTS.")?;
                return Err(error);
            }
        };
        let segments = transcript
            .into_iter()
            .filter(|s| s.enabled && !s.translated_text.trim().is_empty())
            .collect::<Vec<_>>();
        if segments.is_empty() {
            queue.fail(
                claimed.job.id,
                "INVALID_INPUT",
                "Không có bản dịch để tổng hợp.",
            )?;
            return Err(CoreError::InvalidInput("TTS segments"));
        }
        let definitions = segments
            .iter()
            .map(|s| {
                let assignment = self
                    .tts
                    .resolve_for_segment(s.project_id, s.id, s.speaker_id)?
                    .ok_or(CoreError::NotFound("voice assignment"))?;
                if assignment.provider_id != execution.provider.provider_id
                    || !matches!(assignment.voice_id.as_str(), "alloy" | "nova")
                {
                    return Err(CoreError::InvalidInput("voice assignment provider"));
                }
                let identity = identity(
                    &s.translation_hash,
                    &assignment.provider_id,
                    &assignment.voice_id,
                    &execution.model,
                    execution.speed,
                );
                Ok(NewTtsSegmentRun {
                    id: Uuid::new_v4(),
                    project_id: s.project_id,
                    stage_run_id: claimed.job.stage_run_id,
                    segment_id: s.id,
                    cache_identity: identity,
                    provider_id: assignment.provider_id,
                    voice_id: assignment.voice_id,
                    target_duration_ms: s.end_ms - s.start_ms,
                })
            })
            .collect::<Result<Vec<_>, CoreError>>();
        let definitions = match definitions {
            Ok(value) => value,
            Err(error) => {
                queue.fail(
                    claimed.job.id,
                    error.code(),
                    "Chưa có giọng đọc hợp lệ cho mọi đoạn thoại.",
                )?;
                return Err(error);
            }
        };
        if let Err(error) = self.tts.recover_stage(claimed.job.stage_run_id) {
            queue.fail(
                claimed.job.id,
                error.code(),
                "Không thể phục hồi trạng thái TTS.",
            )?;
            return Err(error);
        }
        let mut runs = match self.tts.list_for_stage(claimed.job.stage_run_id) {
            Ok(value) => value,
            Err(error) => {
                queue.fail(
                    claimed.job.id,
                    error.code(),
                    "Không thể đọc trạng thái TTS.",
                )?;
                return Err(error);
            }
        };
        if runs.is_empty() {
            runs = match self.tts.insert_runs(&definitions) {
                Ok(value) => value,
                Err(error) => {
                    queue.fail(
                        claimed.job.id,
                        error.code(),
                        "Không thể tạo trạng thái TTS.",
                    )?;
                    return Err(error);
                }
            }
        } else if runs.len() != definitions.len()
            || runs
                .iter()
                .zip(&definitions)
                .any(|(a, b)| a.segment_id != b.segment_id || a.cache_identity != b.cache_identity)
        {
            queue.fail(claimed.job.id, "INVALID_INPUT", "TTS cache không còn khớp.")?;
            return Err(CoreError::InvalidInput("stale TTS runs"));
        }
        let secret = match self.credentials.get(&execution.credential) {
            Ok(v) => v,
            Err(e) => {
                queue.fail(claimed.job.id, e.code(), "Không có credential TTS.")?;
                return Err(e);
            }
        };
        let root = self.layout.project_root(claimed.job.project_id)?;
        let client = match self
            .workers
            .client_for_stage(StageName::Synthesize, &root, &[])
        {
            Ok(value) => value,
            Err(error) => {
                queue.fail(
                    claimed.job.id,
                    error.code(),
                    "Không thể khởi động bộ máy TTS.",
                )?;
                return Err(error);
            }
        };
        let mut artifacts = Vec::new();
        let mut warnings = Vec::new();
        let mut failures = 0;
        for run in &mut runs {
            if run.status == TtsRunStatus::Completed {
                let verified = run.artifact_id.is_some_and(|id| {
                    self.artifacts.verify(id).ok() == Some(ArtifactVerification::Verified)
                });
                if verified {
                    continue;
                }
                *run = self.tts.invalidate_cached(run.id)?;
            }
            if claimed.cancellation.is_cancelled() {
                queue.acknowledge_interruption(claimed.job.id)?;
                return Err(CoreError::WorkerExecution);
            }
            let active = self.tts.mark_running(run.id)?;
            let segment = segments
                .iter()
                .find(|s| s.id == active.segment_id)
                .ok_or(CoreError::NotFound("segment"))?;
            let request = worker_request(
                claimed.job.project_id,
                segment.id,
                &segment.translated_text,
                &active.voice_id,
                execution,
                false,
                secret.expose(),
            );
            let response = client.run(&request, claimed.cancellation.clone()).await;
            if claimed.cancellation.is_cancelled() {
                self.tts.reset_pending(active.id)?;
                queue.acknowledge_interruption(claimed.job.id)?;
                return Err(CoreError::WorkerExecution);
            }
            match response.map(|v| v.artifacts) {
                Ok(descriptors) => match self.consume(active.clone(), &descriptors) {
                    Ok((updated, artifact)) => {
                        if let Some(code) = &updated.warning_code {
                            warnings.push((updated.segment_id, code.clone()))
                        }
                        *run = updated;
                        artifacts.push(artifact)
                    }
                    Err(_) => {
                        self.tts.fail(active.id, "INVALID_TTS_AUDIO")?;
                        failures += 1
                    }
                },
                Err(_) => {
                    self.tts.fail(active.id, "TTS_PROVIDER_FAILED")?;
                    failures += 1
                }
            }
        }
        if failures > 0 {
            queue.fail(
                claimed.job.id,
                "TTS_PARTIAL_FAILURE",
                "Một số đoạn thoại chưa được tổng hợp.",
            )?;
            return Err(CoreError::WorkerExecution);
        }
        queue.complete(
            claimed.job.id,
            &runs
                .iter()
                .filter_map(|run| run.artifact_id)
                .collect::<Vec<_>>(),
        )?;
        Ok(TtsExecutionResult {
            runs,
            artifacts,
            warnings,
        })
    }
    pub async fn preview(
        &self,
        project_id: Uuid,
        segment_id: Uuid,
        execution: &TtsExecutionRequest,
        cancel: CancellationToken,
    ) -> Result<Artifact, CoreError> {
        execution.validate()?;
        let segment = self.transcript.get_segment(project_id, segment_id)?;
        let assignment = self
            .tts
            .resolve_for_segment(project_id, segment_id, segment.speaker_id)?
            .ok_or(CoreError::NotFound("voice assignment"))?;
        if assignment.provider_id != execution.provider.provider_id
            || !matches!(assignment.voice_id.as_str(), "alloy" | "nova")
            || segment.translated_text.trim().is_empty()
        {
            return Err(CoreError::InvalidInput("preview voice assignment"));
        }
        let secret = self.credentials.get(&execution.credential)?;
        let root = self.layout.project_root(project_id)?;
        let client = self
            .workers
            .client_for_stage(StageName::VoicePreview, &root, &[])?;
        let response = client
            .run(
                &worker_request(
                    project_id,
                    segment.id,
                    &segment.translated_text,
                    &assignment.voice_id,
                    execution,
                    true,
                    secret.expose(),
                ),
                cancel,
            )
            .await
            .map_err(|_| CoreError::WorkerExecution)?;
        let descriptor = response
            .artifacts
            .first()
            .ok_or(CoreError::InvalidInput("preview artifact"))?;
        if response.artifacts.len() != 1 || descriptor.r#type != "preview" {
            return Err(CoreError::InvalidInput("preview artifact"));
        }
        self.register(
            project_id,
            descriptor,
            ArtifactKind::Preview,
            StageName::VoicePreview,
        )
        .map(|(a, _)| a)
    }
    fn consume(
        &self,
        run: TtsSegmentRun,
        descriptors: &[crate::workers::ArtifactOutput],
    ) -> Result<(TtsSegmentRun, Artifact), CoreError> {
        if descriptors.len() != 1 || descriptors[0].r#type != "tts" {
            return Err(CoreError::InvalidInput("TTS artifact"));
        }
        let (artifact, duration) = self.register(
            run.project_id,
            &descriptors[0],
            ArtifactKind::Tts,
            StageName::Synthesize,
        )?;
        let fit = duration_fit(duration, run.target_duration_ms)?;
        let updated = self.tts.complete(
            run.id,
            artifact.id,
            duration,
            fit.playback_rate,
            fit.warning_code,
        );
        let updated = match updated {
            Ok(value) => value,
            Err(error) => {
                self.artifacts.unregister(artifact.id)?;
                return Err(error);
            }
        };
        Ok((updated, artifact))
    }
    fn register(
        &self,
        project_id: Uuid,
        d: &crate::workers::ArtifactOutput,
        kind: ArtifactKind,
        stage: StageName,
    ) -> Result<(Artifact, u64), CoreError> {
        let relative = ProjectRelativePath::parse(&d.relative_path)?;
        let path = self.layout.resolve_existing(project_id, &relative)?;
        let (hash, size) = sha256_file(&path)?;
        if hash != d.sha256 || size != d.size_bytes || size > 32 * 1024 * 1024 {
            return Err(CoreError::ArtifactIntegrity);
        }
        let duration = parse_wav_duration(&fs::read(&path)?)?;
        let artifact = self.artifacts.register_existing(
            project_id,
            kind,
            &d.relative_path,
            stage,
            &d.metadata,
        )?;
        Ok((artifact, duration))
    }
}
fn identity(hash: &Option<String>, provider: &str, voice: &str, model: &str, speed: f64) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"VIETDUB_TTS_SEGMENT_CACHE\0");
    for value in [hash.as_deref().unwrap_or(""), provider, voice, model] {
        hasher.update((value.len() as u64).to_be_bytes());
        hasher.update(value.as_bytes());
    }
    hasher.update(speed.to_bits().to_be_bytes());
    format!("{:x}", hasher.finalize())
}
fn worker_request(
    project_id: Uuid,
    segment_id: Uuid,
    text: &str,
    voice: &str,
    e: &TtsExecutionRequest,
    preview: bool,
    secret: &str,
) -> WorkerRequest {
    let mut r = WorkerRequest::new(
        if preview {
            "synthesize_preview"
        } else {
            "synthesize"
        },
        project_id,
        if preview { "previews" } else { "audio/tts" },
    );
    r.input.insert("tts".into(),json!({"schema_version":1,"segment_id":segment_id,"text":text,"voice_id":voice,"speed":e.speed,"preview":preview}));
    for (k, v) in [
        ("provider_id", e.provider.provider_id.clone()),
        ("endpoint", e.endpoint.clone()),
        ("model", e.model.clone()),
        ("api_key", secret.to_owned()),
    ] {
        r.config.insert(k.into(), Value::String(v));
    }
    r.config
        .insert("cloud_consent".into(), Value::Bool(e.cloud_consent));
    r.config
        .insert("max_attempts".into(), Value::from(e.max_attempts));
    r
}
fn parse_wav_duration(data: &[u8]) -> Result<u64, CoreError> {
    if data.len() < 44 || &data[..4] != b"RIFF" || &data[8..12] != b"WAVE" {
        return Err(CoreError::InvalidInput("TTS WAV"));
    }
    let mut offset = 12usize;
    let (mut channels, mut rate, mut bits, mut size) = (0u64, 0u64, 0u64, 0u64);
    while offset + 8 <= data.len() {
        let chunk = &data[offset..offset + 4];
        let length = u32::from_le_bytes(
            data[offset + 4..offset + 8]
                .try_into()
                .map_err(|_| CoreError::InvalidInput("TTS WAV"))?,
        ) as usize;
        let body = offset + 8;
        if body + length > data.len() {
            return Err(CoreError::InvalidInput("TTS WAV"));
        }
        if chunk == b"fmt " && length >= 16 {
            let format = u16::from_le_bytes(
                data[body..body + 2]
                    .try_into()
                    .map_err(|_| CoreError::InvalidInput("TTS WAV"))?,
            );
            if format != 1 {
                return Err(CoreError::InvalidInput("TTS WAV PCM"));
            }
            channels = u16::from_le_bytes(data[body + 2..body + 4].try_into().unwrap()) as u64;
            rate = u32::from_le_bytes(data[body + 4..body + 8].try_into().unwrap()) as u64;
            bits = u16::from_le_bytes(data[body + 14..body + 16].try_into().unwrap()) as u64
        } else if chunk == b"data" {
            size = length as u64
        }
        offset = body + length + (length % 2)
    }
    if channels == 0 || rate == 0 || bits == 0 || size == 0 {
        return Err(CoreError::InvalidInput("TTS WAV metadata"));
    }
    Ok(size * 8 * 1000 / (channels * rate * bits))
}
