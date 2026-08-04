use std::{fs, path::PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::{
    domain::{
        Artifact, ArtifactKind, ArtifactVerification, CacheDescriptor, CoreError, NewSegment,
        NewStageRun, Segment, StageName, StageScope,
    },
    jobs::{ClaimedJob, PersistentQueue},
    workers::{RequiredModel, WorkerClientError, WorkerManager, WorkerRequest},
};

use super::{sha256_file, ArtifactRegistry, ProjectLayout, ProjectRelativePath, TranscriptService};

const ASR_RESULT_SCHEMA: &str = include_str!("../../../../../schemas/asr-result.schema.json");
const MAX_TRANSCRIPT_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct AsrExecutionRequest {
    pub audio_artifact_id: Uuid,
    pub primary_model_id: String,
    pub primary_model_path: PathBuf,
    pub fallback_model_id: Option<String>,
    pub fallback_model_path: Option<PathBuf>,
    pub language: String,
    pub region: Option<AsrRegion>,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct AsrRegion {
    pub start_ms: u64,
    pub end_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct AsrExecutionResult {
    pub transcript_artifact: Artifact,
    pub segments: Vec<Segment>,
    pub warnings: Vec<String>,
    pub review_job_id: Uuid,
}

#[derive(Clone)]
pub struct AsrPipelineService {
    artifacts: ArtifactRegistry,
    layout: ProjectLayout,
    transcript: TranscriptService,
    workers: WorkerManager,
}

impl AsrPipelineService {
    pub fn new(
        artifacts: ArtifactRegistry,
        layout: ProjectLayout,
        transcript: TranscriptService,
        workers: WorkerManager,
    ) -> Self {
        Self {
            artifacts,
            layout,
            transcript,
            workers,
        }
    }

    pub async fn execute_claimed(
        &self,
        queue: &PersistentQueue,
        claimed: ClaimedJob,
        execution: &AsrExecutionRequest,
    ) -> Result<AsrExecutionResult, CoreError> {
        if claimed.job.job_type != StageName::Transcribe {
            return Err(CoreError::InvalidInput("ASR job stage"));
        }

        let result = self
            .transcribe(
                claimed.job.project_id,
                execution,
                claimed.cancellation.clone(),
                queue,
                claimed.job.id,
            )
            .await;
        let (transcript_artifact, segments, warnings) = match result {
            Ok(result) => result,
            Err(error) => {
                if claimed.cancellation.is_cancelled() {
                    queue.acknowledge_interruption(claimed.job.id)?;
                } else {
                    queue.fail(
                        claimed.job.id,
                        error.code(),
                        "Không thể hoàn tất bước nhận dạng giọng nói.",
                    )?;
                }
                return Err(error);
            }
        };

        if claimed.cancellation.is_cancelled() {
            queue.acknowledge_interruption(claimed.job.id)?;
            return Err(CoreError::WorkerExecution);
        }

        queue.complete(claimed.job.id, &[transcript_artifact.id])?;
        let review_stage = transcript_review_stage(
            claimed.job.project_id,
            &transcript_artifact.sha256,
            &execution.primary_model_id,
        );
        let review_job = queue.create_review_checkpoint(&review_stage, claimed.job.priority)?;

        Ok(AsrExecutionResult {
            transcript_artifact,
            segments,
            warnings,
            review_job_id: review_job.id,
        })
    }

    async fn transcribe(
        &self,
        project_id: Uuid,
        execution: &AsrExecutionRequest,
        cancellation: CancellationToken,
        queue: &PersistentQueue,
        job_id: Uuid,
    ) -> Result<(Artifact, Vec<Segment>, Vec<String>), CoreError> {
        validate_execution(execution)?;
        let audio = self.artifacts.get(execution.audio_artifact_id)?;
        if audio.project_id != project_id || audio.kind != ArtifactKind::OriginalAudio {
            return Err(CoreError::InvalidInput("ASR audio artifact"));
        }
        if self.artifacts.verify(audio.id)? != ArtifactVerification::Verified {
            return Err(CoreError::ArtifactIntegrity);
        }

        let project_root = self.layout.project_root(project_id)?;
        let required_models = required_models(execution);
        let client = self.workers.client_for_stage(
            StageName::Transcribe,
            &project_root,
            &required_models,
        )?;
        let mut request = WorkerRequest::new("transcribe", project_id, "metadata");
        request
            .input
            .insert("audio_path".into(), Value::String(audio.relative_path));
        request.input.insert(
            "model_id".into(),
            Value::String(execution.primary_model_id.clone()),
        );
        request
            .input
            .insert("language".into(), Value::String(execution.language.clone()));
        request.config.insert(
            "primary_model_path".into(),
            Value::String(path_string(&execution.primary_model_path)?),
        );
        if let (Some(model_id), Some(model_path)) =
            (&execution.fallback_model_id, &execution.fallback_model_path)
        {
            request
                .config
                .insert("fallback_model_id".into(), Value::String(model_id.clone()));
            request.config.insert(
                "fallback_model_path".into(),
                Value::String(path_string(model_path)?),
            );
        }
        if let Some(region) = execution.region {
            request
                .config
                .insert("region_start_ms".into(), Value::from(region.start_ms));
            request
                .config
                .insert("region_end_ms".into(), Value::from(region.end_ms));
        }

        let run = client
            .run_with_progress(&request, cancellation, |event| {
                let progress = (5.0 + f64::from(event.progress) * 0.9).min(95.0);
                let _ = queue.update_progress(job_id, progress);
            })
            .await
            .map_err(map_worker_error)?;
        let descriptor = run
            .artifacts
            .iter()
            .find(|artifact| artifact.r#type == "transcript")
            .ok_or(CoreError::InvalidInput("ASR transcript artifact"))?;
        if run
            .artifacts
            .iter()
            .filter(|artifact| artifact.r#type == "transcript")
            .count()
            != 1
        {
            return Err(CoreError::InvalidInput("ASR transcript artifacts"));
        }

        let relative = ProjectRelativePath::parse(&descriptor.relative_path)?;
        let transcript_path = self.layout.resolve_existing(project_id, &relative)?;
        let (sha256, size_bytes) = sha256_file(&transcript_path)?;
        if sha256 != descriptor.sha256
            || size_bytes != descriptor.size_bytes
            || size_bytes > MAX_TRANSCRIPT_BYTES
        {
            return Err(CoreError::ArtifactIntegrity);
        }
        let raw_segments = read_asr_result(&transcript_path)?;
        let new_segments = raw_segments
            .into_iter()
            .enumerate()
            .map(|(sequence, segment)| NewSegment {
                id: Uuid::new_v4(),
                project_id,
                sequence: sequence as u32,
                start_ms: segment.start_ms,
                end_ms: segment.end_ms,
                source_text: segment.text,
                speaker_id: None,
                asr_confidence: Some(segment.confidence),
            })
            .collect::<Vec<_>>();
        let artifact = self.artifacts.register_existing(
            project_id,
            ArtifactKind::Transcript,
            &descriptor.relative_path,
            StageName::Transcribe,
            &descriptor.metadata,
        )?;
        if artifact.sha256 != descriptor.sha256 || artifact.size_bytes != descriptor.size_bytes {
            self.artifacts.unregister(artifact.id)?;
            return Err(CoreError::ArtifactIntegrity);
        }
        let persistence_result = if let Some(region) = execution.region {
            self.transcript.replace_regional_asr_results(
                project_id,
                region.start_ms,
                region.end_ms,
                new_segments,
            )
        } else {
            self.transcript.import_asr_results(project_id, new_segments)
        };
        let segments = match persistence_result {
            Ok(segments) => segments,
            Err(error) => {
                self.artifacts.unregister(artifact.id)?;
                return Err(error);
            }
        };
        Ok((artifact, segments, run.warnings))
    }
}

fn map_worker_error(error: WorkerClientError) -> CoreError {
    match error {
        WorkerClientError::InvalidRequest => CoreError::InvalidInput("ASR worker request"),
        WorkerClientError::Spawn(_) | WorkerClientError::Write(_) => CoreError::WorkerStartFailed,
        WorkerClientError::Timeout => CoreError::WorkerTimeout,
        WorkerClientError::Cancelled => CoreError::WorkerCancelled,
        WorkerClientError::ProcessExited => CoreError::WorkerProcessExited,
        WorkerClientError::WorkerFailed { error_code, .. } => match error_code.as_str() {
            "MODEL_NOT_AVAILABLE" => CoreError::AsrModelUnavailable,
            "MODEL_LOAD_FAILED" => CoreError::AsrModelLoadFailed,
            "TRANSCRIPTION_FAILED" => CoreError::AsrTranscriptionFailed,
            "ASR_INTERNAL_ERROR" => CoreError::AsrInternalFailure,
            _ => CoreError::WorkerReportedFailure,
        },
        WorkerClientError::MessageTooLarge => CoreError::WorkerMessageTooLarge,
        WorkerClientError::InvalidMessage => CoreError::WorkerInvalidMessage,
        WorkerClientError::RequestMismatch => CoreError::WorkerRequestMismatch,
        WorkerClientError::VersionMismatch => CoreError::WorkerVersionMismatch,
        WorkerClientError::DuplicateTerminal => CoreError::WorkerDuplicateTerminal,
        WorkerClientError::MissingTerminal => CoreError::WorkerMissingTerminal,
    }
}

#[derive(Debug, Deserialize)]
struct AsrResultFile {
    schema_version: u32,
    segments: Vec<AsrResultSegment>,
}

#[derive(Debug, Deserialize)]
struct AsrResultSegment {
    start_ms: u64,
    end_ms: u64,
    text: String,
    confidence: f64,
}

fn read_asr_result(path: &std::path::Path) -> Result<Vec<AsrResultSegment>, CoreError> {
    let content = fs::read(path)?;
    let value: Value =
        serde_json::from_slice(&content).map_err(|_| CoreError::InvalidInput("ASR result JSON"))?;
    let schema: Value = serde_json::from_str(ASR_RESULT_SCHEMA)
        .map_err(|_| CoreError::InvalidInput("ASR result schema"))?;
    jsonschema::validator_for(&schema)
        .map_err(|_| CoreError::InvalidInput("ASR result schema"))?
        .validate(&value)
        .map_err(|_| CoreError::InvalidInput("ASR result"))?;
    let result: AsrResultFile =
        serde_json::from_value(value).map_err(|_| CoreError::InvalidInput("ASR result"))?;
    if result.schema_version != 1
        || result.segments.iter().any(|segment| {
            segment.end_ms <= segment.start_ms
                || segment.text.trim().is_empty()
                || !(0.0..=1.0).contains(&segment.confidence)
        })
    {
        return Err(CoreError::InvalidInput("ASR result"));
    }
    Ok(result.segments)
}

fn validate_execution(execution: &AsrExecutionRequest) -> Result<(), CoreError> {
    if execution.primary_model_id.is_empty()
        || execution.language.is_empty()
        || execution.language.len() > 16
        || execution
            .region
            .is_some_and(|region| region.end_ms <= region.start_ms)
        || (execution.primary_model_id.starts_with("funasr:")
            && (execution.fallback_model_id.is_none() || execution.fallback_model_path.is_none()))
    {
        return Err(CoreError::InvalidInput("ASR execution"));
    }
    Ok(())
}

fn required_models(execution: &AsrExecutionRequest) -> Vec<RequiredModel> {
    let mut models = vec![RequiredModel {
        model_id: execution.primary_model_id.clone(),
        root: execution.primary_model_path.clone(),
    }];
    if let (Some(model_id), Some(root)) =
        (&execution.fallback_model_id, &execution.fallback_model_path)
    {
        models.push(RequiredModel {
            model_id: model_id.clone(),
            root: root.clone(),
        });
    }
    models
}

fn path_string(path: &std::path::Path) -> Result<String, CoreError> {
    let canonical = path.canonicalize()?;
    if !canonical.is_dir() {
        return Err(CoreError::InvalidInput("model directory"));
    }
    let value = canonical
        .to_str()
        .ok_or(CoreError::InvalidInput("model directory"))?;
    Ok(normalize_worker_path(value))
}

#[cfg(windows)]
fn normalize_worker_path(value: &str) -> String {
    if let Some(rest) = value.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else {
        value.strip_prefix(r"\\?\").unwrap_or(value).to_owned()
    }
}

#[cfg(not(windows))]
fn normalize_worker_path(value: &str) -> String {
    value.to_owned()
}

fn transcript_review_stage(project_id: Uuid, transcript_hash: &str, model_id: &str) -> NewStageRun {
    let mut metadata = Map::new();
    metadata.insert("checkpoint".into(), json!("transcript_review"));
    let config_hash = format!("{:x}", Sha256::digest(b"transcript-review-v1"));
    let mut stage = NewStageRun::new(
        project_id,
        StageName::TranscriptReview,
        StageScope::Project,
        CacheDescriptor {
            schema_version: 1,
            input_hash: transcript_hash.to_owned(),
            config_hash,
            engine_name: "human-review".into(),
            engine_version: "1".into(),
            model_version: model_id.to_owned(),
            metadata,
        },
        "human-review",
    );
    stage.model_version = model_id.to_owned();
    stage
}

#[cfg(all(test, windows))]
mod tests {
    use super::normalize_worker_path;

    #[test]
    fn worker_paths_use_native_windows_form() {
        assert_eq!(
            normalize_worker_path(r"\\?\D:\models\asr"),
            r"D:\models\asr"
        );
        assert_eq!(
            normalize_worker_path(r"\\?\UNC\server\share\models"),
            r"\\server\share\models"
        );
    }
}
