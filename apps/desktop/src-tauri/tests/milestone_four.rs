use std::{env, fs, path::PathBuf, sync::Arc, time::Duration};

use serde_json::Map;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use vietdub_desktop_lib::{
    domain::{
        CacheDescriptor, CoreError, NewProject, NewSegment, NewStageRun, ReviewStatus,
        SegmentUpdate, StageName, StageScope, StageStatus, TranslationBlockStatus,
        TranslationProviderDisclosure, TranslationResult, WorkflowMode,
    },
    infrastructure::{
        ArtifactRegistry, ProjectLayout, ProjectService, TranscriptService,
        TranslationExecutionRequest, TranslationPipelineService,
    },
    jobs::{InvalidationEngine, PersistentQueue},
    persistence::{
        ArtifactRepository, Database, ModelConsentRepository, ProjectRepository, SegmentRepository,
        StageRunRepository, TranslationRepository,
    },
    security::{CredentialReference, CredentialStore, SecretString},
    workers::WorkerManager,
};

#[derive(Debug)]
struct TestCredentialStore {
    secret: String,
}

impl CredentialStore for TestCredentialStore {
    fn get(&self, _reference: &CredentialReference) -> Result<SecretString, CoreError> {
        SecretString::new(self.secret.clone())
    }
}

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("workspace root")
        .to_path_buf()
}

fn python_311() -> PathBuf {
    if let Ok(program) = env::var("VIETDUB_PYTHON") {
        return PathBuf::from(program);
    }
    repository_root().join(".venv/Scripts/python.exe")
}

fn cache_descriptor(label: &str) -> CacheDescriptor {
    CacheDescriptor {
        schema_version: 1,
        input_hash: format!("{:x}", Sha256::digest(format!("input-{label}").as_bytes())),
        config_hash: format!("{:x}", Sha256::digest(format!("config-{label}").as_bytes())),
        engine_name: label.into(),
        engine_version: "1".into(),
        model_version: "test".into(),
        metadata: Map::new(),
    }
}

fn stage(project_id: Uuid, name: StageName, scope: StageScope) -> NewStageRun {
    let mut stage = NewStageRun::new(
        project_id,
        name,
        scope,
        cache_descriptor(name.as_str()),
        name.as_str(),
    );
    stage.model_version = "test".into();
    stage
}

fn execution() -> TranslationExecutionRequest {
    TranslationExecutionRequest {
        disclosure: TranslationProviderDisclosure {
            provider_id: "openai-compatible".into(),
            display_name: "Test cloud provider".into(),
            sends_data_off_device: true,
        },
        endpoint: Some("https://api.example.com/v1/chat/completions".into()),
        model: "test-translation-model".into(),
        local_model_path: None,
        credential: Some(
            CredentialReference::new("vietdub", "translation-test").expect("credential ref"),
        ),
        cloud_consent: true,
        source_language: "zh".into(),
        target_language: "vi".into(),
        block_size: 1,
        max_attempts: 2,
    }
}

struct Harness {
    _temporary: tempfile::TempDir,
    database: Database,
    layout: ProjectLayout,
    transcript: TranscriptService,
    translations: TranslationRepository,
    pipeline: TranslationPipelineService,
    queue: PersistentQueue,
    project_id: Uuid,
}

fn harness(database: Database, temporary: tempfile::TempDir, sources: &[&str]) -> Harness {
    let layout = ProjectLayout::new(temporary.path().join("projects")).expect("layout");
    let projects = ProjectService::new(ProjectRepository::new(database.clone()), layout.clone());
    let project = projects
        .create(&NewProject::chinese_to_vietnamese(
            "Translation integration",
            WorkflowMode::Subtitles,
        ))
        .expect("project");
    let transcript = TranscriptService::new(
        SegmentRepository::new(database.clone()),
        InvalidationEngine::new(database.clone()),
    );
    let new_segments = sources
        .iter()
        .enumerate()
        .map(|(index, source)| NewSegment {
            id: Uuid::new_v4(),
            project_id: project.id,
            sequence: index as u32,
            start_ms: index as u64 * 1_000,
            end_ms: (index as u64 + 1) * 1_000,
            source_text: (*source).into(),
            speaker_id: None,
            asr_confidence: Some(0.9),
        })
        .collect();
    transcript
        .import_asr_results(project.id, new_segments)
        .expect("segments");
    transcript
        .approve_transcript(project.id)
        .expect("reviewed transcript");
    let translations = TranslationRepository::new(database.clone());
    let artifacts =
        ArtifactRegistry::new(ArtifactRepository::new(database.clone()), layout.clone());
    let workers = WorkerManager::new(
        python_311(),
        repository_root().join("tests/fixtures/translation_workers"),
        ModelConsentRepository::new(database.clone()),
    );
    let pipeline = TranslationPipelineService::new(
        artifacts,
        layout.clone(),
        transcript.clone(),
        translations.clone(),
        workers,
        Arc::new(TestCredentialStore {
            secret: "milestone-four-secret-token".into(),
        }),
    );
    let queue = PersistentQueue::new(database.clone(), 2).expect("queue");
    Harness {
        _temporary: temporary,
        database,
        layout,
        transcript,
        translations,
        pipeline,
        queue,
        project_id: project.id,
    }
}

#[tokio::test]
async fn partial_blocks_survive_failure_and_retry_reuses_completed_work() {
    let temporary = tempfile::tempdir().expect("temporary");
    let harness = harness(
        Database::in_memory().expect("database"),
        temporary,
        &["第一句", "FAIL_ONCE 第二句"],
    );
    let queued = harness
        .queue
        .enqueue(
            &stage(
                harness.project_id,
                StageName::Translate,
                StageScope::Project,
            ),
            10,
        )
        .expect("enqueue");
    let claimed = harness.queue.claim_next().unwrap().unwrap();
    let error = harness
        .pipeline
        .execute_claimed(&harness.queue, claimed, &execution())
        .await
        .expect_err("second block fails once");
    assert!(matches!(error, CoreError::WorkerExecution));
    let failed_job = harness.queue.get(queued.id).expect("failed job");
    let first_attempt = harness
        .translations
        .list_for_stage(failed_job.stage_run_id)
        .expect("first blocks");
    assert_eq!(first_attempt[0].status, TranslationBlockStatus::Completed);
    assert_eq!(first_attempt[1].status, TranslationBlockStatus::Failed);
    assert!(!harness
        .transcript
        .get_transcript(harness.project_id)
        .unwrap()[0]
        .translated_text
        .is_empty());

    let retry = harness.queue.retry(queued.id).expect("retry");
    let retry_claim = harness.queue.claim_next().unwrap().unwrap();
    assert_eq!(retry_claim.job.id, retry.id);
    let result = harness
        .pipeline
        .execute_claimed(&harness.queue, retry_claim, &execution())
        .await
        .expect("retry succeeds");
    assert_eq!(result.artifacts.len(), 1, "completed first block is reused");
    assert!(result.disclosure.sends_data_off_device);
    assert!(result
        .blocks
        .iter()
        .all(|block| block.status == TranslationBlockStatus::Completed));
    assert_eq!(harness.queue.in_flight_count().unwrap(), 0);
    let review = StageRunRepository::new(harness.database.clone())
        .find_latest_by_status(
            harness.project_id,
            StageName::TranslationReview,
            StageStatus::ReviewRequired,
        )
        .unwrap()
        .expect("translation review checkpoint");
    assert_eq!(
        harness.queue.get(result.review_job_id).unwrap().status,
        vietdub_desktop_lib::domain::JobStatus::Completed
    );
    harness
        .queue
        .complete_translation_review(harness.project_id)
        .unwrap();
    assert_eq!(
        StageRunRepository::new(harness.database.clone())
            .get(review.stage_id)
            .unwrap()
            .status,
        StageStatus::Completed
    );
    assert!(harness
        .transcript
        .get_transcript(harness.project_id)
        .unwrap()
        .iter()
        .all(|segment| !segment.translated_text.is_empty()));
}

#[test]
fn restart_recovery_resets_only_running_blocks_and_preserves_completed_blocks() {
    let temporary = tempfile::tempdir().expect("temporary");
    let database_path = temporary.path().join("restart.sqlite3");
    let project_id;
    let stage_id;
    {
        let database = Database::open(&database_path).expect("database");
        let projects = ProjectRepository::new(database.clone());
        project_id = projects
            .insert(
                Uuid::new_v4(),
                &NewProject::chinese_to_vietnamese("Recovery", WorkflowMode::Subtitles),
            )
            .unwrap()
            .id;
        stage_id = StageRunRepository::new(database.clone())
            .insert(&stage(
                project_id,
                StageName::Translate,
                StageScope::Project,
            ))
            .unwrap()
            .stage_id;
        let repository = TranslationRepository::new(database);
        let blocks = repository
            .insert_blocks(&[
                vietdub_desktop_lib::domain::NewTranslationBlock {
                    id: Uuid::new_v4(),
                    project_id,
                    stage_run_id: stage_id,
                    block_index: 0,
                    segment_ids: vec![Uuid::new_v4()],
                    source_hash: "a".repeat(64),
                },
                vietdub_desktop_lib::domain::NewTranslationBlock {
                    id: Uuid::new_v4(),
                    project_id,
                    stage_run_id: stage_id,
                    block_index: 1,
                    segment_ids: vec![Uuid::new_v4()],
                    source_hash: "b".repeat(64),
                },
            ])
            .unwrap();
        repository.mark_running(blocks[0].id).unwrap();
    }
    let reopened = Database::open(&database_path).expect("reopen");
    let repository = TranslationRepository::new(reopened);
    assert_eq!(repository.recover_stage(stage_id).unwrap(), 1);
    let recovered = repository.list_for_stage(stage_id).unwrap();
    assert_eq!(recovered[0].status, TranslationBlockStatus::Pending);
    assert_eq!(recovered[1].status, TranslationBlockStatus::Pending);
}

#[tokio::test]
async fn cloud_secret_is_not_persisted_in_database_or_project_files() {
    let temporary = tempfile::tempdir().expect("temporary");
    let database_path = temporary.path().join("secrets.sqlite3");
    let harness = harness(
        Database::open(&database_path).expect("database"),
        temporary,
        &["Alice 来了"],
    );
    harness
        .translations
        .add_locked_name(harness.project_id, "Alice")
        .expect("locked name");
    let queued = harness
        .queue
        .enqueue(
            &stage(
                harness.project_id,
                StageName::Translate,
                StageScope::Project,
            ),
            10,
        )
        .unwrap();
    let claimed = harness.queue.claim_next().unwrap().unwrap();
    harness
        .pipeline
        .execute_claimed(&harness.queue, claimed, &execution())
        .await
        .expect("translation");
    assert_eq!(
        harness.queue.get(queued.id).unwrap().status,
        vietdub_desktop_lib::domain::JobStatus::Completed
    );
    assert!(!String::from_utf8_lossy(&fs::read(&database_path).unwrap())
        .contains("milestone-four-secret-token"));
    for entry in fs::read_dir(harness.layout.project_root(harness.project_id).unwrap()).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_dir() {
            for child in fs::read_dir(entry.path()).unwrap() {
                let child = child.unwrap();
                if child.file_type().unwrap().is_file() {
                    assert!(!String::from_utf8_lossy(&fs::read(child.path()).unwrap())
                        .contains("milestone-four-secret-token"));
                }
            }
        }
    }
}

#[tokio::test]
async fn cancelling_translation_does_not_cancel_another_project_job() {
    let slow_temp = tempfile::tempdir().unwrap();
    let fast_temp = tempfile::tempdir().unwrap();
    let database = Database::in_memory().unwrap();
    let slow = harness(database.clone(), slow_temp, &["SLEEP"]);
    let fast = harness(database, fast_temp, &["快速"]);
    let slow_job = slow
        .queue
        .enqueue(
            &stage(slow.project_id, StageName::Translate, StageScope::Project),
            20,
        )
        .unwrap();
    let fast_job = slow
        .queue
        .enqueue(
            &stage(fast.project_id, StageName::Translate, StageScope::Project),
            10,
        )
        .unwrap();
    let slow_claim = slow.queue.claim_next().unwrap().unwrap();
    let fast_claim = slow.queue.claim_next().unwrap().unwrap();
    let slow_pipeline = slow.pipeline.clone();
    let slow_queue = slow.queue.clone();
    let slow_task = tokio::spawn(async move {
        slow_pipeline
            .execute_claimed(&slow_queue, slow_claim, &execution())
            .await
    });
    tokio::time::sleep(Duration::from_millis(150)).await;
    slow.queue.cancel(slow_job.id).unwrap();
    let fast_result = fast
        .pipeline
        .execute_claimed(&slow.queue, fast_claim, &execution())
        .await;
    assert!(fast_result.is_ok());
    assert!(slow_task.await.unwrap().is_err());
    assert_eq!(
        slow.queue.get(slow_job.id).unwrap().status,
        vietdub_desktop_lib::domain::JobStatus::Cancelled
    );
    assert_eq!(
        slow.queue.get(fast_job.id).unwrap().status,
        vietdub_desktop_lib::domain::JobStatus::Completed
    );
}

#[test]
fn manual_translation_edit_invalidates_only_the_affected_downstream_scope() {
    let temporary = tempfile::tempdir().unwrap();
    let harness = harness(Database::in_memory().unwrap(), temporary, &["第一", "第二"]);
    let segments = harness
        .transcript
        .get_transcript(harness.project_id)
        .unwrap();
    let stages = StageRunRepository::new(harness.database.clone());
    let affected = stages
        .insert(&stage(
            harness.project_id,
            StageName::Synthesize,
            StageScope::Segment(segments[0].id),
        ))
        .unwrap();
    let preserved = stages
        .insert(&stage(
            harness.project_id,
            StageName::Synthesize,
            StageScope::Segment(segments[1].id),
        ))
        .unwrap();
    let translate = stages
        .insert(&stage(
            harness.project_id,
            StageName::Translate,
            StageScope::Segment(segments[0].id),
        ))
        .unwrap();
    for run in [&affected, &preserved, &translate] {
        stages
            .set_status(run.stage_id, StageStatus::Completed, 100.0, None, None)
            .unwrap();
    }
    harness
        .transcript
        .update_segment(
            harness.project_id,
            segments[0].id,
            &SegmentUpdate {
                translated_text: Some("Bản dịch sửa".into()),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(
        stages.get(affected.stage_id).unwrap().status,
        StageStatus::Invalidated
    );
    assert_eq!(
        stages.get(preserved.stage_id).unwrap().status,
        StageStatus::Completed
    );
    assert_eq!(
        stages.get(translate.stage_id).unwrap().status,
        StageStatus::Completed
    );
    assert_eq!(
        harness
            .transcript
            .get_transcript(harness.project_id)
            .unwrap()[0]
            .review_status,
        ReviewStatus::Approved
    );
}

#[test]
fn strict_result_validation_rejects_duplicate_missing_empty_and_locked_name_changes() {
    let first = Uuid::new_v4();
    let second = Uuid::new_v4();
    let duplicate = TranslationResult {
        schema_version: 1,
        translations: vec![
            vietdub_desktop_lib::domain::TranslationItem {
                id: first,
                text: "one".into(),
            },
            vietdub_desktop_lib::domain::TranslationItem {
                id: first,
                text: "duplicate".into(),
            },
        ],
    };
    assert!(matches!(
        duplicate.validate_exact(&[first, second]),
        Err(CoreError::InvalidTranslationOutput)
    ));
    let empty = TranslationResult {
        schema_version: 1,
        translations: vec![vietdub_desktop_lib::domain::TranslationItem {
            id: first,
            text: " ".into(),
        }],
    };
    assert!(empty.validate_exact(&[first]).is_err());
    let changed_name = TranslationResult {
        schema_version: 1,
        translations: vec![vietdub_desktop_lib::domain::TranslationItem {
            id: first,
            text: "Cô ấy đến".into(),
        }],
    };
    let sources = [(first, "Alice 来了".to_string())].into_iter().collect();
    assert!(changed_name
        .validate_locked_names(&sources, &["Alice".into()])
        .is_err());
}
