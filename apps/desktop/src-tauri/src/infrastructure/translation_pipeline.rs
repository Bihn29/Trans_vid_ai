use std::{collections::HashMap, fs, path::PathBuf, sync::Arc};

use serde::Serialize;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    domain::{
        Artifact, ArtifactKind, CacheDescriptor, CoreError, NewStageRun, NewTranslationBlock,
        StageName, StageScope, TranslationBlock, TranslationBlockStatus,
        TranslationProviderDisclosure, TranslationResult, MAX_TRANSLATION_BLOCK_SEGMENTS,
    },
    jobs::{ClaimedJob, PersistentQueue},
    persistence::TranslationRepository,
    security::{CredentialReference, CredentialStore},
    workers::{RequiredModel, WorkerManager, WorkerRequest},
};

use super::{sha256_file, ArtifactRegistry, ProjectLayout, ProjectRelativePath, TranscriptService};

const RESULT_SCHEMA: &str = include_str!("../../../../../schemas/translation-result.schema.json");
const MAX_RESULT_BYTES: u64 = 4 * 1024 * 1024;
const CONTEXT_SEGMENTS: usize = 3;

#[derive(Debug, Clone)]
pub struct TranslationExecutionRequest {
    pub disclosure: TranslationProviderDisclosure,
    pub endpoint: Option<String>,
    pub model: String,
    pub local_model_path: Option<PathBuf>,
    pub credential: Option<CredentialReference>,
    pub cloud_consent: bool,
    pub source_language: String,
    pub target_language: String,
    pub block_size: usize,
    pub max_attempts: u8,
}

impl TranslationExecutionRequest {
    pub fn validate(&self) -> Result<(), CoreError> {
        let is_cloud = self.disclosure.sends_data_off_device;
        if self.disclosure.provider_id.is_empty()
            || self.disclosure.display_name.is_empty()
            || self.model.is_empty()
            || self.source_language.is_empty()
            || self.target_language.is_empty()
            || self.block_size == 0
            || self.block_size > MAX_TRANSLATION_BLOCK_SEGMENTS
            || !(1..=3).contains(&self.max_attempts)
            || (is_cloud
                && (!self.cloud_consent
                    || self.credential.is_none()
                    || self.endpoint.as_deref().is_none_or(str::is_empty)))
            || (!is_cloud && (self.credential.is_some() || self.cloud_consent))
            || (is_cloud && self.local_model_path.is_some())
            || (!is_cloud && self.local_model_path.is_none())
        {
            return Err(CoreError::InvalidInput("translation execution"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TranslationExecutionResult {
    pub blocks: Vec<TranslationBlock>,
    pub artifacts: Vec<Artifact>,
    pub review_job_id: Uuid,
    pub disclosure: TranslationProviderDisclosure,
}

#[derive(Clone)]
pub struct TranslationPipelineService {
    artifacts: ArtifactRegistry,
    layout: ProjectLayout,
    transcript: TranscriptService,
    translations: TranslationRepository,
    workers: WorkerManager,
    credentials: Arc<dyn CredentialStore>,
}

impl TranslationPipelineService {
    pub fn new(
        artifacts: ArtifactRegistry,
        layout: ProjectLayout,
        transcript: TranscriptService,
        translations: TranslationRepository,
        workers: WorkerManager,
        credentials: Arc<dyn CredentialStore>,
    ) -> Self {
        Self {
            artifacts,
            layout,
            transcript,
            translations,
            workers,
            credentials,
        }
    }

    pub async fn execute_claimed(
        &self,
        queue: &PersistentQueue,
        claimed: ClaimedJob,
        execution: &TranslationExecutionRequest,
    ) -> Result<TranslationExecutionResult, CoreError> {
        if claimed.job.job_type != StageName::Translate {
            return Err(CoreError::InvalidInput("translation job stage"));
        }
        if let Err(error) = execution.validate() {
            return self.fail_job(queue, claimed.job.id, error);
        }
        let project_id = claimed.job.project_id;
        let transcript = self.transcript.get_transcript(project_id)?;
        if transcript
            .iter()
            .any(|segment| segment.review_status != crate::domain::ReviewStatus::Approved)
        {
            return self.fail_job(
                queue,
                claimed.job.id,
                CoreError::InvalidInput("unreviewed transcript"),
            );
        }
        let segments = transcript
            .into_iter()
            .filter(|segment| segment.enabled && !segment.source_text.trim().is_empty())
            .collect::<Vec<_>>();
        if segments.is_empty() {
            return self.fail_job(
                queue,
                claimed.job.id,
                CoreError::InvalidInput("translation segments"),
            );
        }
        let glossary = self.translations.list_glossary(project_id)?;
        let locked_names = self.translations.list_locked_names(project_id)?;
        let definitions = build_block_definitions(
            project_id,
            claimed.job.stage_run_id,
            &segments,
            execution,
            &glossary,
            &locked_names,
        )?;

        self.translations.recover_stage(claimed.job.stage_run_id)?;
        let mut blocks = self.translations.list_for_stage(claimed.job.stage_run_id)?;
        if blocks.is_empty() {
            blocks = self.translations.insert_blocks(&definitions)?;
        } else if !definitions_match(&definitions, &blocks) {
            return self.fail_job(
                queue,
                claimed.job.id,
                CoreError::InvalidInput("stale translation blocks"),
            );
        }

        let secret = execution
            .credential
            .as_ref()
            .map(|reference| self.credentials.get(reference))
            .transpose();
        let secret = match secret {
            Ok(value) => value,
            Err(error) => return self.fail_job(queue, claimed.job.id, error),
        };
        let project_root = self.layout.project_root(project_id)?;
        let required_models = execution
            .local_model_path
            .as_ref()
            .map(|root| {
                vec![RequiredModel {
                    model_id: execution.model.clone(),
                    root: root.clone(),
                }]
            })
            .unwrap_or_default();
        let client =
            self.workers
                .client_for_stage(StageName::Translate, &project_root, &required_models);
        let client = match client {
            Ok(value) => value,
            Err(error) => return self.fail_job(queue, claimed.job.id, error),
        };
        let mut artifacts = Vec::new();
        let sources = segments
            .iter()
            .map(|segment| (segment.id, segment.source_text.clone()))
            .collect::<HashMap<_, _>>();
        let locked_values = locked_names
            .iter()
            .map(|name| name.value.clone())
            .collect::<Vec<_>>();

        let total_blocks = blocks.len();
        for (block_position, block) in blocks.iter_mut().enumerate() {
            if block.status == TranslationBlockStatus::Completed {
                continue;
            }
            if claimed.cancellation.is_cancelled() {
                queue.acknowledge_interruption(claimed.job.id)?;
                return Err(CoreError::WorkerExecution);
            }
            let running = self.translations.mark_running(block.id)?;
            let request = match build_worker_request(
                project_id,
                &running,
                &segments,
                execution,
                &glossary,
                &locked_names,
                secret.as_ref().map(|value| value.expose()),
            ) {
                Ok(request) => request,
                Err(error) => return self.fail_job(queue, claimed.job.id, error),
            };
            let run = client.run(&request, claimed.cancellation.clone()).await;
            let run = match run {
                Ok(value) => value,
                Err(_) if claimed.cancellation.is_cancelled() => {
                    self.translations.reset_pending(running.id)?;
                    queue.acknowledge_interruption(claimed.job.id)?;
                    return Err(CoreError::WorkerExecution);
                }
                Err(_) => {
                    self.translations.fail(
                        running.id,
                        "TRANSLATION_PROVIDER_FAILED",
                        "Không thể hoàn tất khối dịch.",
                    )?;
                    queue.fail(
                        claimed.job.id,
                        "TRANSLATION_PROVIDER_FAILED",
                        "Không thể hoàn tất bước dịch.",
                    )?;
                    return Err(CoreError::WorkerExecution);
                }
            };
            if claimed.cancellation.is_cancelled() {
                self.translations.reset_pending(running.id)?;
                queue.acknowledge_interruption(claimed.job.id)?;
                return Err(CoreError::WorkerExecution);
            }
            let artifact = match self.consume_result(
                project_id,
                &running,
                &run.artifacts,
                &sources,
                &locked_values,
            ) {
                Ok(value) => value,
                Err(error) => {
                    self.translations.fail(
                        running.id,
                        error.code(),
                        "Kết quả dịch không hợp lệ.",
                    )?;
                    queue.fail(claimed.job.id, error.code(), "Kết quả dịch không hợp lệ.")?;
                    return Err(error);
                }
            };
            artifacts.push(artifact);
            *block = self.translations.get(running.id)?;
            let progress = ((block_position + 1) as f64 / total_blocks as f64 * 95.0).min(95.0);
            queue.update_progress(claimed.job.id, progress)?;
        }

        queue.complete(
            claimed.job.id,
            &artifacts
                .iter()
                .map(|artifact| artifact.id)
                .collect::<Vec<_>>(),
        )?;
        let aggregate_hash = aggregate_result_hash(&blocks);
        let review_stage = translation_review_stage(project_id, &aggregate_hash, execution);
        let review_job = queue.create_review_checkpoint(&review_stage, claimed.job.priority)?;
        Ok(TranslationExecutionResult {
            blocks,
            artifacts,
            review_job_id: review_job.id,
            disclosure: execution.disclosure.clone(),
        })
    }

    fn consume_result(
        &self,
        project_id: Uuid,
        block: &TranslationBlock,
        descriptors: &[crate::workers::ArtifactOutput],
        sources: &HashMap<Uuid, String>,
        locked_names: &[String],
    ) -> Result<Artifact, CoreError> {
        if descriptors.len() != 1 || descriptors[0].r#type != "translation_block" {
            return Err(CoreError::InvalidTranslationOutput);
        }
        let descriptor = &descriptors[0];
        let relative = ProjectRelativePath::parse(&descriptor.relative_path)?;
        let path = self.layout.resolve_existing(project_id, &relative)?;
        let (sha256, size_bytes) = sha256_file(&path)?;
        if sha256 != descriptor.sha256
            || size_bytes != descriptor.size_bytes
            || size_bytes > MAX_RESULT_BYTES
        {
            return Err(CoreError::ArtifactIntegrity);
        }
        let result = read_translation_result(&path)?;
        result.validate_exact(&block.segment_ids)?;
        result.validate_locked_names(sources, locked_names)?;
        let artifact = self.artifacts.register_existing(
            project_id,
            ArtifactKind::Translation,
            &descriptor.relative_path,
            StageName::Translate,
            &descriptor.metadata,
        )?;
        if artifact.sha256 != descriptor.sha256 || artifact.size_bytes != descriptor.size_bytes {
            self.artifacts.unregister(artifact.id)?;
            return Err(CoreError::ArtifactIntegrity);
        }
        if let Err(error) = self.translations.complete(block.id, &result) {
            self.artifacts.unregister(artifact.id)?;
            return Err(error);
        }
        Ok(artifact)
    }

    fn fail_job<T>(
        &self,
        queue: &PersistentQueue,
        job_id: Uuid,
        error: CoreError,
    ) -> Result<T, CoreError> {
        queue.fail(
            job_id,
            error.code(),
            "Khối dịch không còn khớp với transcript.",
        )?;
        Err(error)
    }
}

fn build_block_definitions(
    project_id: Uuid,
    stage_run_id: Uuid,
    segments: &[crate::domain::Segment],
    execution: &TranslationExecutionRequest,
    glossary: &[crate::domain::GlossaryEntry],
    locked_names: &[crate::domain::LockedProperName],
) -> Result<Vec<NewTranslationBlock>, CoreError> {
    segments
        .chunks(execution.block_size)
        .enumerate()
        .map(|(index, block)| {
            let source_hash = block_source_hash(block, execution, glossary, locked_names)?;
            Ok(NewTranslationBlock {
                id: Uuid::new_v4(),
                project_id,
                stage_run_id,
                block_index: index as u32,
                segment_ids: block.iter().map(|segment| segment.id).collect(),
                source_hash,
            })
        })
        .collect()
}

fn block_source_hash(
    segments: &[crate::domain::Segment],
    execution: &TranslationExecutionRequest,
    glossary: &[crate::domain::GlossaryEntry],
    locked_names: &[crate::domain::LockedProperName],
) -> Result<String, CoreError> {
    let value = json!({
        "schema_version": 1,
        "provider_id": execution.disclosure.provider_id,
        "model": execution.model,
        "source_language": execution.source_language,
        "target_language": execution.target_language,
        "segments": segments.iter().map(|segment| json!({"id": segment.id, "source_hash": segment.source_hash})).collect::<Vec<_>>(),
        "glossary": glossary,
        "locked_names": locked_names,
    });
    let bytes = serde_json::to_vec(&value)
        .map_err(|_| CoreError::InvalidInput("translation block identity"))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn definitions_match(definitions: &[NewTranslationBlock], blocks: &[TranslationBlock]) -> bool {
    definitions.len() == blocks.len()
        && definitions.iter().zip(blocks).all(|(definition, block)| {
            definition.block_index == block.block_index
                && definition.segment_ids == block.segment_ids
                && definition.source_hash == block.source_hash
        })
}

fn build_worker_request(
    project_id: Uuid,
    block: &TranslationBlock,
    all_segments: &[crate::domain::Segment],
    execution: &TranslationExecutionRequest,
    glossary: &[crate::domain::GlossaryEntry],
    locked_names: &[crate::domain::LockedProperName],
    secret: Option<&str>,
) -> Result<WorkerRequest, CoreError> {
    let positions = all_segments
        .iter()
        .enumerate()
        .map(|(index, segment)| (segment.id, index))
        .collect::<HashMap<_, _>>();
    let first = positions[&block.segment_ids[0]];
    let last = positions[block.segment_ids.last().expect("validated non-empty block")];
    let segment_value = |segment: &crate::domain::Segment| json!({"id": segment.id, "source_text": segment.source_text});
    let context_before = &all_segments[first.saturating_sub(CONTEXT_SEGMENTS)..first];
    let context_after =
        &all_segments[last + 1..(last + 1 + CONTEXT_SEGMENTS).min(all_segments.len())];
    let block_segments = block
        .segment_ids
        .iter()
        .map(|id| segment_value(&all_segments[positions[id]]))
        .collect::<Vec<_>>();
    let mut request = WorkerRequest::new("translate_block", project_id, "metadata");
    request.input.insert(
        "block".into(),
        json!({
            "schema_version": 1,
            "source_language": execution.source_language,
            "target_language": execution.target_language,
            "segments": block_segments,
            "context_before": context_before.iter().map(segment_value).collect::<Vec<_>>(),
            "context_after": context_after.iter().map(segment_value).collect::<Vec<_>>(),
            "glossary": glossary,
            "locked_names": locked_names.iter().map(|name| &name.value).collect::<Vec<_>>(),
        }),
    );
    request.config.insert(
        "provider_id".into(),
        Value::String(execution.disclosure.provider_id.clone()),
    );
    request
        .config
        .insert("model".into(), Value::String(execution.model.clone()));
    if let Some(model_path) = &execution.local_model_path {
        request.config.insert(
            "model_path".into(),
            Value::String(worker_model_path(model_path)?),
        );
    }
    request
        .config
        .insert("max_attempts".into(), Value::from(execution.max_attempts));
    request
        .config
        .insert("cloud_consent".into(), Value::Bool(execution.cloud_consent));
    if let Some(endpoint) = &execution.endpoint {
        request
            .config
            .insert("endpoint".into(), Value::String(endpoint.clone()));
    }
    if let Some(secret) = secret {
        request
            .config
            .insert("api_key".into(), Value::String(secret.to_owned()));
    }
    Ok(request)
}

fn worker_model_path(path: &std::path::Path) -> Result<String, CoreError> {
    let canonical = path.canonicalize()?;
    if !canonical.is_dir() {
        return Err(CoreError::InvalidInput("translation model directory"));
    }
    let value = canonical
        .to_str()
        .ok_or(CoreError::InvalidInput("translation model directory"))?;
    #[cfg(windows)]
    {
        if let Some(rest) = value.strip_prefix(r"\\?\UNC\") {
            return Ok(format!(r"\\{rest}"));
        }
        Ok(value.strip_prefix(r"\\?\").unwrap_or(value).to_owned())
    }
    #[cfg(not(windows))]
    Ok(value.to_owned())
}

fn read_translation_result(path: &std::path::Path) -> Result<TranslationResult, CoreError> {
    let content = fs::read(path)?;
    let value: Value =
        serde_json::from_slice(&content).map_err(|_| CoreError::InvalidTranslationOutput)?;
    let schema: Value =
        serde_json::from_str(RESULT_SCHEMA).map_err(|_| CoreError::InvalidTranslationOutput)?;
    jsonschema::validator_for(&schema)
        .map_err(|_| CoreError::InvalidTranslationOutput)?
        .validate(&value)
        .map_err(|_| CoreError::InvalidTranslationOutput)?;
    serde_json::from_value(value).map_err(|_| CoreError::InvalidTranslationOutput)
}

fn aggregate_result_hash(blocks: &[TranslationBlock]) -> String {
    let mut hasher = Sha256::new();
    for block in blocks {
        hasher.update(block.source_hash.as_bytes());
        if let Some(result) = &block.result {
            hasher.update(serde_json::to_vec(result).unwrap_or_default());
        }
    }
    format!("{:x}", hasher.finalize())
}

fn translation_review_stage(
    project_id: Uuid,
    input_hash: &str,
    execution: &TranslationExecutionRequest,
) -> NewStageRun {
    let config_hash = format!(
        "{:x}",
        Sha256::digest(
            format!(
                "{}\0{}\0{}",
                execution.disclosure.provider_id, execution.model, execution.target_language
            )
            .as_bytes()
        )
    );
    let mut stage = NewStageRun::new(
        project_id,
        StageName::TranslationReview,
        StageScope::Project,
        CacheDescriptor {
            schema_version: 1,
            input_hash: input_hash.into(),
            config_hash,
            engine_name: "human-translation-review".into(),
            engine_version: "1".into(),
            model_version: execution.model.clone(),
            metadata: Map::new(),
        },
        "human-translation-review",
    );
    stage.model_version = execution.model.clone();
    stage
}
