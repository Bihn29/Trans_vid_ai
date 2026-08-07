use serde_json::Map;
use sha2::{Digest, Sha256};
use std::{env, fs, path::PathBuf, sync::Arc};
use uuid::Uuid;
use vietdub_desktop_lib::{
    domain::{
        duration_fit, CacheDescriptor, CoreError, NewProject, NewSegment, NewStageRun,
        NewTtsSegmentRun, SegmentUpdate, StageName, StageScope, TtsRunStatus, VoiceDescriptor,
        VoiceScope, WorkflowMode,
    },
    infrastructure::{
        ArtifactRegistry, ProjectLayout, ProjectService, TranscriptService, TtsExecutionRequest,
        TtsPipelineService,
    },
    jobs::{InvalidationEngine, PersistentQueue},
    persistence::{
        ArtifactRepository, Database, ModelConsentRepository, ProjectRepository, SegmentRepository,
        TtsRepository,
    },
    security::{CredentialReference, CredentialStore, SecretString},
    workers::WorkerManager,
};
#[derive(Debug)]
struct Store;
impl CredentialStore for Store {
    fn get(&self, _: &CredentialReference) -> Result<SecretString, CoreError> {
        SecretString::new("tts-secret-not-persisted")
    }
}
fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .unwrap()
        .to_path_buf()
}
fn python() -> PathBuf {
    env::var("VIETDUB_PYTHON")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("python"))
}
fn stage(project: Uuid) -> NewStageRun {
    let cache = CacheDescriptor {
        schema_version: 1,
        input_hash: format!("{:x}", Sha256::digest(b"tts-input")),
        config_hash: format!("{:x}", Sha256::digest(b"tts-config")),
        engine_name: "tts".into(),
        engine_version: "1".into(),
        model_version: "tts-1".into(),
        metadata: Map::new(),
    };
    let mut s = NewStageRun::new(
        project,
        StageName::Synthesize,
        StageScope::Project,
        cache,
        "tts",
    );
    s.model_version = "tts-1".into();
    s
}
fn execution() -> TtsExecutionRequest {
    TtsExecutionRequest {
        provider: VoiceDescriptor {
            provider_id: "openai-compatible".into(),
            voice_id: "alloy".into(),
            display_name: "Alloy".into(),
            language: "multilingual".into(),
            sends_data_off_device: true,
            approved: true,
        },
        endpoint: "https://api.example.com/v1/audio/speech".into(),
        model: "tts-1".into(),
        credential: CredentialReference::new("vietdub", "tts").unwrap(),
        cloud_consent: true,
        speed: 1.0,
        max_attempts: 2,
    }
}
struct H {
    _temp: tempfile::TempDir,
    transcript: TranscriptService,
    tts: TtsRepository,
    pipeline: TtsPipelineService,
    queue: PersistentQueue,
    project: Uuid,
    segments: Vec<Uuid>,
}
fn setup(texts: &[&str]) -> H {
    let temp = tempfile::tempdir().unwrap();
    let db = Database::in_memory().unwrap();
    let layout = ProjectLayout::new(temp.path().join("projects")).unwrap();
    let project = ProjectService::new(ProjectRepository::new(db.clone()), layout.clone())
        .create(&NewProject::chinese_to_vietnamese(
            "TTS",
            WorkflowMode::Dubbed,
        ))
        .unwrap();
    let transcript = TranscriptService::new(
        SegmentRepository::new(db.clone()),
        InvalidationEngine::new(db.clone()),
    );
    let input = texts
        .iter()
        .enumerate()
        .map(|(i, _text)| NewSegment {
            id: Uuid::new_v4(),
            project_id: project.id,
            sequence: i as u32,
            start_ms: i as u64 * 1000,
            end_ms: (i as u64 + 1) * 1000,
            source_text: format!("src {i}"),
            speaker_id: Some(if i % 2 == 0 {
                Uuid::from_u128(1)
            } else {
                Uuid::from_u128(2)
            }),
            asr_confidence: Some(0.9),
        })
        .collect::<Vec<_>>();
    let imported = transcript.import_asr_results(project.id, input).unwrap();
    transcript.approve_transcript(project.id).unwrap();
    for (s, t) in imported.iter().zip(texts) {
        transcript
            .update_segment(
                project.id,
                s.id,
                &SegmentUpdate {
                    translated_text: Some((*t).into()),
                    ..Default::default()
                },
            )
            .unwrap();
    }
    let tts = TtsRepository::new(db.clone());
    tts.set_assignment(
        project.id,
        VoiceScope::Speaker(Uuid::from_u128(1)),
        "openai-compatible",
        "alloy",
    )
    .unwrap();
    tts.set_assignment(
        project.id,
        VoiceScope::Speaker(Uuid::from_u128(2)),
        "openai-compatible",
        "nova",
    )
    .unwrap();
    let artifacts = ArtifactRegistry::new(ArtifactRepository::new(db.clone()), layout.clone());
    let workers = WorkerManager::new(
        python(),
        root().join("tests/fixtures/tts_workers"),
        ModelConsentRepository::new(db.clone()),
    );
    let pipeline = TtsPipelineService::new(
        artifacts,
        layout.clone(),
        transcript.clone(),
        tts.clone(),
        workers,
        Arc::new(Store),
    );
    let queue = PersistentQueue::new(db.clone(), 2).unwrap();
    H {
        _temp: temp,
        transcript,
        tts,
        pipeline,
        queue,
        project: project.id,
        segments: imported.iter().map(|s| s.id).collect(),
    }
}
#[tokio::test]
async fn two_speakers_route_to_distinct_voices_and_measure_audio() {
    let h = setup(&["xin chÃ o", "táº¡m biá»‡t"]);
    let job = h.queue.enqueue(&stage(h.project), 10).unwrap();
    let result = h
        .pipeline
        .execute_claimed(
            &h.queue,
            h.queue.claim_next().unwrap().unwrap(),
            &execution(),
        )
        .await
        .unwrap();
    assert_eq!(result.artifacts.len(), 2);
    assert_ne!(result.runs[0].cache_identity, result.runs[1].cache_identity);
    let segments = h.transcript.get_transcript(h.project).unwrap();
    assert_eq!(segments[0].voice_id.as_deref(), Some("alloy"));
    assert_eq!(segments[1].voice_id.as_deref(), Some("nova"));
    assert!(segments
        .iter()
        .all(|s| s.estimated_duration_ms == Some(1000) && s.audio_artifact_id.is_some()));
    assert_eq!(
        h.queue.get(job.id).unwrap().status,
        vietdub_desktop_lib::domain::JobStatus::Completed
    )
}
#[tokio::test]
async fn segment_failure_isolated_and_retry_reuses_success() {
    let h = setup(&["completed", "FAIL_ONCE retry"]);
    let first = h.queue.enqueue(&stage(h.project), 10).unwrap();
    assert!(h
        .pipeline
        .execute_claimed(
            &h.queue,
            h.queue.claim_next().unwrap().unwrap(),
            &execution()
        )
        .await
        .is_err());
    let runs = h
        .tts
        .list_for_stage(h.queue.get(first.id).unwrap().stage_run_id)
        .unwrap();
    assert_eq!(runs[0].status, TtsRunStatus::Completed);
    assert_eq!(runs[1].status, TtsRunStatus::Failed);
    let retry = h.queue.retry(first.id).unwrap();
    let result = h
        .pipeline
        .execute_claimed(
            &h.queue,
            h.queue.claim_next().unwrap().unwrap(),
            &execution(),
        )
        .await
        .unwrap();
    assert_eq!(result.artifacts.len(), 1);
    assert_eq!(
        h.queue.get(retry.id).unwrap().status,
        vietdub_desktop_lib::domain::JobStatus::Completed
    )
}
#[tokio::test]
async fn preview_is_verified_and_does_not_replace_segment_audio() {
    let h = setup(&["preview"]);
    let before = h
        .transcript
        .get_segment(h.project, h.segments[0])
        .unwrap()
        .audio_artifact_id;
    let preview = h
        .pipeline
        .preview(
            h.project,
            h.segments[0],
            &execution(),
            tokio_util::sync::CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(
        preview.kind,
        vietdub_desktop_lib::domain::ArtifactKind::Preview
    );
    assert_eq!(
        h.transcript
            .get_segment(h.project, h.segments[0])
            .unwrap()
            .audio_artifact_id,
        before
    )
}
#[test]
fn duration_policy_requests_shorter_text_outside_safe_stretch() {
    let fit = duration_fit(2500, 1000).unwrap();
    assert_eq!(fit.playback_rate, 1.2);
    assert_eq!(fit.warning_code, Some("SHORTEN_TRANSLATION"));
    let slow = duration_fit(500, 1000).unwrap();
    assert_eq!(slow.warning_code, Some("EXCESSIVE_SLOWDOWN"))
}
#[test]
fn assignments_are_project_isolated_and_segment_overrides_speaker() {
    let h = setup(&["one"]);
    h.tts
        .set_assignment(
            h.project,
            VoiceScope::Segment(h.segments[0]),
            "openai-compatible",
            "nova",
        )
        .unwrap();
    let segment = h.transcript.get_segment(h.project, h.segments[0]).unwrap();
    assert_eq!(
        h.tts
            .resolve_for_segment(h.project, segment.id, segment.speaker_id)
            .unwrap()
            .unwrap()
            .voice_id,
        "nova"
    );
    assert!(h
        .tts
        .resolve_for_segment(Uuid::new_v4(), segment.id, segment.speaker_id)
        .unwrap()
        .is_none())
}

#[tokio::test]
async fn corrupt_cached_audio_is_rejected_and_regenerated() {
    let h = setup(&["cache integrity"]);
    h.queue.enqueue(&stage(h.project), 10).unwrap();
    let first = h
        .pipeline
        .execute_claimed(
            &h.queue,
            h.queue.claim_next().unwrap().unwrap(),
            &execution(),
        )
        .await
        .unwrap();
    let old_artifact = first.artifacts.first().unwrap();
    let old_path = h
        ._temp
        .path()
        .join("projects")
        .join(h.project.to_string())
        .join(PathBuf::from(&old_artifact.relative_path));
    fs::write(old_path, b"corrupt").unwrap();

    h.queue.enqueue(&stage(h.project), 10).unwrap();
    let regenerated = h
        .pipeline
        .execute_claimed(
            &h.queue,
            h.queue.claim_next().unwrap().unwrap(),
            &execution(),
        )
        .await
        .unwrap();

    assert_eq!(regenerated.artifacts.len(), 1);
    assert_ne!(regenerated.artifacts[0].id, old_artifact.id);
    assert_eq!(regenerated.runs[0].status, TtsRunStatus::Completed);
}

#[test]
fn restart_recovery_resets_only_running_tts_segments() {
    let h = setup(&["restart"]);
    let job = h.queue.enqueue(&stage(h.project), 10).unwrap();
    let inserted = h
        .tts
        .insert_runs(&[NewTtsSegmentRun {
            id: Uuid::new_v4(),
            project_id: h.project,
            stage_run_id: job.stage_run_id,
            segment_id: h.segments[0],
            cache_identity: "a".repeat(64),
            provider_id: "openai-compatible".into(),
            voice_id: "alloy".into(),
            target_duration_ms: 1_000,
        }])
        .unwrap();
    h.tts.mark_running(inserted[0].id).unwrap();

    assert_eq!(h.tts.recover_stage(job.stage_run_id).unwrap(), 1);
    assert_eq!(
        h.tts.get(inserted[0].id).unwrap().status,
        TtsRunStatus::Pending
    );
}
