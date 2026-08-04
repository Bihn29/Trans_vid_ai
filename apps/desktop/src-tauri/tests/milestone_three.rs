use std::{env, fs, path::PathBuf, process::Command as StdCommand};

use serde_json::{json, Map};
use sha2::{Digest, Sha256};
use uuid::Uuid;
use vietdub_desktop_lib::domain::{
    check_transcript_quality, compute_text_hash, ArtifactKind, ArtifactVerification,
    CacheDescriptor, CoreError, JobStatus, ModelManifest, NewProject, NewSegment, NewStageRun,
    ReviewStatus, Segment, SegmentUpdate, SegmentWarning, StageName, StageScope, StageStatus,
    WorkflowMode,
};
use vietdub_desktop_lib::infrastructure::{
    ArtifactRegistry, AsrExecutionRequest, AsrPipelineService, AsrRegion, ProjectLayout,
    ProjectService, TranscriptService,
};
use vietdub_desktop_lib::jobs::{InvalidationEngine, PersistentQueue};
use vietdub_desktop_lib::persistence::{
    ArtifactRepository, Database, ModelConsentRepository, ProjectRepository, SegmentRepository,
    StageRunRepository,
};
use vietdub_desktop_lib::workers::{RequiredModel, WorkerManager};

fn setup() -> (Database, TranscriptService, ModelConsentRepository, Uuid) {
    let database = Database::in_memory().expect("open in-memory database");
    let segments = SegmentRepository::new(database.clone());
    let transcript = TranscriptService::new(segments, InvalidationEngine::new(database.clone()));
    let model_consents = ModelConsentRepository::new(database.clone());

    let projects = vietdub_desktop_lib::persistence::ProjectRepository::new(database.clone());
    let project = projects
        .insert(
            Uuid::new_v4(),
            &NewProject::chinese_to_vietnamese("Test Project", WorkflowMode::Subtitles),
        )
        .expect("create project");

    (database, transcript, model_consents, project.id)
}

fn make_segments(project_id: Uuid) -> Vec<NewSegment> {
    vec![
        NewSegment {
            id: Uuid::new_v4(),
            project_id,
            sequence: 0,
            start_ms: 0,
            end_ms: 2000,
            source_text: "你好世界".into(),
            speaker_id: None,
            asr_confidence: Some(0.95),
        },
        NewSegment {
            id: Uuid::new_v4(),
            project_id,
            sequence: 1,
            start_ms: 2000,
            end_ms: 4500,
            source_text: "这是一个测试".into(),
            speaker_id: None,
            asr_confidence: Some(0.88),
        },
        NewSegment {
            id: Uuid::new_v4(),
            project_id,
            sequence: 2,
            start_ms: 4500,
            end_ms: 7000,
            source_text: "谢谢大家".into(),
            speaker_id: None,
            asr_confidence: Some(0.92),
        },
    ]
}

#[test]
fn migration_v3_creates_segments_and_model_consents_tables() {
    let database = Database::in_memory().expect("open in-memory database");
    let segments = SegmentRepository::new(database.clone());
    let consents = ModelConsentRepository::new(database);

    assert_eq!(segments.list_by_project(Uuid::new_v4()).unwrap(), vec![]);
    assert_eq!(consents.list_all().unwrap(), vec![]);
}

#[test]
fn segment_crud_and_bulk_insert() {
    let (_db, transcript, _consents, project_id) = setup();
    let segments = make_segments(project_id);

    let imported = transcript
        .import_asr_results(project_id, segments.clone())
        .expect("import segments");
    assert_eq!(imported.len(), 3);

    let all = transcript
        .get_transcript(project_id)
        .expect("get transcript");
    assert_eq!(all.len(), 3);
    assert_eq!(all[0].sequence, 0);
    assert_eq!(all[1].sequence, 1);
    assert_eq!(all[2].sequence, 2);

    let single = transcript
        .get_segment(project_id, imported[0].id)
        .expect("get segment");
    assert_eq!(single.source_text, "你好世界");
}

#[test]
fn import_replaces_existing_segments() {
    let (_db, transcript, _consents, project_id) = setup();
    let segments = make_segments(project_id);

    transcript
        .import_asr_results(project_id, segments)
        .expect("first import");

    let new_segments = vec![NewSegment {
        id: Uuid::new_v4(),
        project_id,
        sequence: 0,
        start_ms: 0,
        end_ms: 5000,
        source_text: "replaced".into(),
        speaker_id: None,
        asr_confidence: Some(0.99),
    }];

    let imported = transcript
        .import_asr_results(project_id, new_segments)
        .expect("second import");
    assert_eq!(imported.len(), 1);

    let all = transcript
        .get_transcript(project_id)
        .expect("get transcript");
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].source_text, "replaced");
}

#[test]
fn import_rejects_overlapping_segments() {
    let (_db, transcript, _consents, project_id) = setup();

    let segments = vec![
        NewSegment {
            id: Uuid::new_v4(),
            project_id,
            sequence: 0,
            start_ms: 0,
            end_ms: 3000,
            source_text: "first".into(),
            speaker_id: None,
            asr_confidence: Some(0.9),
        },
        NewSegment {
            id: Uuid::new_v4(),
            project_id,
            sequence: 1,
            start_ms: 2500,
            end_ms: 5000,
            source_text: "overlap".into(),
            speaker_id: None,
            asr_confidence: Some(0.9),
        },
    ];

    let error = transcript
        .import_asr_results(project_id, segments)
        .expect_err("reject overlap");
    assert!(matches!(error, CoreError::SegmentOverlap));
}

#[test]
fn split_preserves_timestamp_invariants() {
    let (_db, transcript, _consents, project_id) = setup();
    let segments = make_segments(project_id);
    let imported = transcript
        .import_asr_results(project_id, segments)
        .expect("import");

    let (first, second) = transcript
        .split_segment(project_id, imported[1].id, 3000)
        .expect("split");

    assert!(first.end_ms > first.start_ms);
    assert!(second.end_ms > second.start_ms);
    assert_eq!(first.start_ms, 2000);
    assert_eq!(first.end_ms, 3000);
    assert_eq!(second.start_ms, 3000);
    assert_eq!(second.end_ms, 4500);

    let all = transcript.get_transcript(project_id).expect("get all");
    assert_eq!(all.len(), 4);
    for (index, segment) in all.iter().enumerate() {
        assert_eq!(segment.sequence, index as u32);
    }
}

#[test]
fn split_rejects_invalid_split_point() {
    let (_db, transcript, _consents, project_id) = setup();
    let segments = make_segments(project_id);
    let imported = transcript
        .import_asr_results(project_id, segments)
        .expect("import");

    let error = transcript
        .split_segment(project_id, imported[0].id, 0)
        .expect_err("reject start boundary");
    assert!(matches!(error, CoreError::InvalidInput(_)));

    let error = transcript
        .split_segment(project_id, imported[0].id, 2000)
        .expect_err("reject end boundary");
    assert!(matches!(error, CoreError::InvalidInput(_)));
}

#[test]
fn merge_concatenates_text_and_updates_timestamps() {
    let (_db, transcript, _consents, project_id) = setup();
    let segments = make_segments(project_id);
    let imported = transcript
        .import_asr_results(project_id, segments)
        .expect("import");

    let merged = transcript
        .merge_segments(project_id, imported[0].id, imported[1].id)
        .expect("merge");

    assert_eq!(merged.start_ms, 0);
    assert_eq!(merged.end_ms, 4500);
    assert!(merged.source_text.contains("你好世界"));
    assert!(merged.source_text.contains("这是一个测试"));

    let all = transcript.get_transcript(project_id).expect("get all");
    assert_eq!(all.len(), 2);
}

#[test]
fn qc_detects_overlap_empty_long_repetition_silence_low_confidence() {
    let id1 = Uuid::new_v4();
    let id2 = Uuid::new_v4();
    let id3 = Uuid::new_v4();
    let id4 = Uuid::new_v4();
    let id5 = Uuid::new_v4();

    let make = |id: Uuid, seq: u32, start: u64, end: u64, text: &str, conf: f64| -> Segment {
        Segment {
            id,
            project_id: Uuid::new_v4(),
            sequence: seq,
            start_ms: start,
            end_ms: end,
            source_text: text.into(),
            translated_text: String::new(),
            speaker_id: None,
            voice_id: None,
            asr_confidence: Some(conf),
            estimated_duration_ms: None,
            target_duration_ms: None,
            playback_rate: 1.0,
            enabled: true,
            review_status: ReviewStatus::Unreviewed,
            source_hash: None,
            translation_hash: None,
            voice_hash: None,
            audio_artifact_id: None,
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
        }
    };

    let segments = vec![
        make(id1, 0, 0, 1000, "  ", 0.1),        // empty + low confidence
        make(id2, 1, 500, 1500, "overlap", 0.9), // overlaps id1
        make(id3, 2, 1500, 20000, "very long segment", 0.9), // long
        make(id4, 3, 26000, 27000, "same", 0.9), // silence gap from id3
        make(id5, 4, 27000, 28000, "same", 0.9), // repetition
    ];

    let warnings = check_transcript_quality(&segments);

    assert!(warnings
        .iter()
        .any(|w| matches!(w, SegmentWarning::EmptyText { .. })));
    assert!(warnings
        .iter()
        .any(|w| matches!(w, SegmentWarning::LowConfidence { .. })));
    assert!(warnings
        .iter()
        .any(|w| matches!(w, SegmentWarning::Overlap { .. })));
    assert!(warnings
        .iter()
        .any(|w| matches!(w, SegmentWarning::LongSegment { .. })));
    assert!(warnings
        .iter()
        .any(|w| matches!(w, SegmentWarning::Silence { .. })));
    assert!(warnings
        .iter()
        .any(|w| matches!(w, SegmentWarning::Repetition { .. })));
}

#[test]
fn source_text_edit_updates_hash() {
    let (_db, transcript, _consents, project_id) = setup();
    let segments = make_segments(project_id);
    let imported = transcript
        .import_asr_results(project_id, segments)
        .expect("import");

    let original_hash = imported[0].source_hash.clone();
    let updated = transcript
        .update_segment_text(project_id, imported[0].id, "edited text".into())
        .expect("update text");

    assert_ne!(updated.source_hash, original_hash);
    assert_eq!(updated.source_text, "edited text");
    assert_eq!(
        updated.source_hash.as_deref(),
        Some(compute_text_hash("edited text").as_str())
    );
}

#[test]
fn approved_transcript_blocks_auto_edit() {
    let (_db, transcript, _consents, project_id) = setup();
    let segments = make_segments(project_id);
    let imported = transcript
        .import_asr_results(project_id, segments)
        .expect("import");

    transcript.approve_transcript(project_id).expect("approve");

    let error = transcript
        .update_segment_text(project_id, imported[0].id, "blocked edit".into())
        .expect_err("block approved edit");
    assert!(matches!(error, CoreError::TranscriptLocked));
}

#[test]
fn model_consent_crud() {
    let (_db, _transcript, consents, _project_id) = setup();

    let manifest = ModelManifest {
        model_id: "funasr:paraformer-zh".into(),
        provider: "funasr".into(),
        display_name: "FunASR Paraformer".into(),
        license: "MIT".into(),
        sends_data_off_device: false,
        estimated_size_bytes: 500_000_000,
        schema_version: 1,
    };

    assert!(!consents
        .has_consent("funasr:paraformer-zh")
        .expect("check before"));

    let consent = consents.insert_consent(&manifest).expect("insert consent");
    assert_eq!(consent.model_id, "funasr:paraformer-zh");
    assert!(!consent.sends_data_off_device);

    assert!(consents
        .has_consent("funasr:paraformer-zh")
        .expect("check after"));

    let all = consents.list_all().expect("list all");
    assert_eq!(all.len(), 1);
}

#[test]
fn model_consent_defaults_to_denied() {
    let database = Database::in_memory().expect("open in-memory database");
    let consents = ModelConsentRepository::new(database);

    assert!(!consents
        .has_consent("funasr:paraformer-zh")
        .expect("no consent"));
}

#[test]
fn transcript_review_checkpoint_approves_all_segments() {
    let (_db, transcript, _consents, project_id) = setup();
    let segments = make_segments(project_id);
    transcript
        .import_asr_results(project_id, segments)
        .expect("import");

    let before = transcript.get_transcript(project_id).expect("get before");
    assert!(before
        .iter()
        .all(|s| s.review_status == ReviewStatus::Unreviewed));

    transcript.approve_transcript(project_id).expect("approve");

    let after = transcript.get_transcript(project_id).expect("get after");
    assert!(after
        .iter()
        .all(|s| s.review_status == ReviewStatus::Approved));
}

#[test]
fn source_text_edit_computes_invalidation_scopes() {
    let (_db, transcript, _consents, project_id) = setup();
    let segments = make_segments(project_id);
    let imported = transcript
        .import_asr_results(project_id, segments)
        .expect("import");

    let scopes = TranscriptService::invalidation_scopes_for_source_edit(&imported[0]);

    assert!(!scopes.is_empty());
    let stage_names: Vec<_> = scopes.iter().map(|(stage, _)| stage.as_str()).collect();
    assert!(stage_names.contains(&"TRANSLATE"));
    assert!(stage_names.contains(&"SYNTHESIZE"));
    assert!(stage_names.contains(&"MIX_AUDIO"));
    assert!(stage_names.contains(&"RENDER"));
    // ASR for other segments should NOT be in the list
    assert!(!stage_names.contains(&"TRANSCRIBE"));
}

fn cache_descriptor(engine: &str, model: &str) -> CacheDescriptor {
    CacheDescriptor {
        schema_version: 1,
        input_hash: "a".repeat(64),
        config_hash: "b".repeat(64),
        engine_name: engine.into(),
        engine_version: "1".into(),
        model_version: model.into(),
        metadata: Map::new(),
    }
}

fn stage_run(project_id: Uuid, stage_name: StageName, scope: StageScope) -> NewStageRun {
    let engine = stage_name.as_str().to_ascii_lowercase();
    let cache = cache_descriptor(&engine, "test-model");
    let mut stage = NewStageRun::new(project_id, stage_name, scope, cache, engine);
    stage.model_version = "test-model".into();
    stage
}

#[test]
fn transcript_editor_enforces_project_isolation_and_safe_edge_cases() {
    let (database, transcript, _consents, project_id) = setup();
    let projects = ProjectRepository::new(database);
    let other_project = projects
        .insert(
            Uuid::new_v4(),
            &NewProject::chinese_to_vietnamese("Other", WorkflowMode::Subtitles),
        )
        .expect("create other project");
    let imported = transcript
        .import_asr_results(project_id, make_segments(project_id))
        .expect("import");

    let error = transcript
        .update_segment(
            other_project.id,
            imported[0].id,
            &SegmentUpdate {
                source_text: Some("cross-project".into()),
                ..SegmentUpdate::default()
            },
        )
        .expect_err("cross-project edit is hidden");
    assert!(matches!(error, CoreError::NotFound("segment")));

    transcript
        .update_segment(
            project_id,
            imported[0].id,
            &SegmentUpdate {
                source_text: Some("x".into()),
                ..SegmentUpdate::default()
            },
        )
        .expect("shorten text");
    let overlap_error = transcript
        .update_segment(
            project_id,
            imported[0].id,
            &SegmentUpdate {
                end_ms: Some(2_500),
                ..SegmentUpdate::default()
            },
        )
        .expect_err("timestamp overlap is rejected");
    assert!(matches!(overlap_error, CoreError::SegmentOverlap));
    let speaker_id = Uuid::new_v4();
    let updated = transcript
        .update_segment(
            project_id,
            imported[0].id,
            &SegmentUpdate {
                end_ms: Some(1_800),
                speaker_id: Some(Some(speaker_id)),
                ..SegmentUpdate::default()
            },
        )
        .expect("update timestamp and speaker");
    assert_eq!(updated.end_ms, 1_800);
    assert_eq!(updated.speaker_id, Some(speaker_id));
    let error = transcript
        .split_segment(project_id, imported[0].id, 1000)
        .expect_err("one-character split is rejected");
    assert!(matches!(error, CoreError::InvalidInput(_)));
    assert_eq!(transcript.get_transcript(project_id).unwrap().len(), 3);

    let error = transcript
        .merge_segments(project_id, imported[0].id, imported[2].id)
        .expect_err("non-adjacent merge is rejected");
    assert!(matches!(error, CoreError::InvalidInput(_)));
    assert_eq!(transcript.get_transcript(project_id).unwrap().len(), 3);
}

#[test]
fn failed_replacement_rolls_back_the_existing_transcript() {
    let (_database, transcript, _consents, project_id) = setup();
    transcript
        .import_asr_results(project_id, make_segments(project_id))
        .expect("initial import");
    let replacement = vec![
        NewSegment {
            id: Uuid::new_v4(),
            project_id,
            sequence: 0,
            start_ms: 0,
            end_ms: 1000,
            source_text: "first".into(),
            speaker_id: None,
            asr_confidence: Some(0.9),
        },
        NewSegment {
            id: Uuid::new_v4(),
            project_id,
            sequence: 0,
            start_ms: 1000,
            end_ms: 2000,
            source_text: "duplicate sequence".into(),
            speaker_id: None,
            asr_confidence: Some(0.9),
        },
    ];

    assert!(transcript
        .import_asr_results(project_id, replacement)
        .is_err());
    let preserved = transcript
        .get_transcript(project_id)
        .expect("preserved transcript");
    assert_eq!(preserved.len(), 3);
}

#[test]
fn source_edit_persists_targeted_invalidation() {
    let (database, transcript, _consents, project_id) = setup();
    let imported = transcript
        .import_asr_results(project_id, make_segments(project_id))
        .expect("import");
    let stages = StageRunRepository::new(database);
    let translate = stages
        .insert(&stage_run(
            project_id,
            StageName::Translate,
            StageScope::Segment(imported[0].id),
        ))
        .expect("translation stage");
    stages
        .set_status(
            translate.stage_id,
            StageStatus::Completed,
            100.0,
            None,
            None,
        )
        .expect("complete translation");
    let transcribe = stages
        .insert(&stage_run(
            project_id,
            StageName::Transcribe,
            StageScope::Project,
        ))
        .expect("transcribe stage");
    stages
        .set_status(
            transcribe.stage_id,
            StageStatus::Completed,
            100.0,
            None,
            None,
        )
        .expect("complete ASR");

    transcript
        .update_segment(
            project_id,
            imported[0].id,
            &SegmentUpdate {
                source_text: Some("edited".into()),
                ..SegmentUpdate::default()
            },
        )
        .expect("edit segment");

    assert_eq!(
        stages.get(translate.stage_id).unwrap().status,
        StageStatus::Invalidated
    );
    assert_eq!(
        stages.get(transcribe.stage_id).unwrap().status,
        StageStatus::Completed
    );
}

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("src-tauri is nested under apps/desktop")
        .to_path_buf()
}

fn python_311() -> PathBuf {
    if let Ok(program) = env::var("VIETDUB_PYTHON") {
        return PathBuf::from(program);
    }
    let workspace_python = repository_root()
        .join(".venv")
        .join("Scripts")
        .join("python.exe");
    let output = StdCommand::new(&workspace_python)
        .arg("--version")
        .output()
        .expect("run workspace Python");
    assert!(output.status.success(), "Python 3.11 is required");
    workspace_python
}

fn manifest(model_id: &str, provider: &str) -> ModelManifest {
    ModelManifest {
        model_id: model_id.into(),
        provider: provider.into(),
        display_name: model_id.into(),
        license: "test-only".into(),
        sends_data_off_device: false,
        estimated_size_bytes: 0,
        schema_version: 1,
    }
}

fn install_test_model(root: &std::path::Path, model_id: &str, provider: &str) {
    fs::create_dir_all(root).expect("model directory");
    let weights = b"deterministic model fixture";
    fs::write(root.join("weights.bin"), weights).expect("model weights");
    let manifest = json!({
        "schema_version": 1,
        "model_id": model_id,
        "provider": provider,
        "version": "test-1",
        "license": "test-only",
        "source_url": "https://example.invalid/models/test",
        "files": [{
            "relative_path": "weights.bin",
            "sha256": format!("{:x}", Sha256::digest(weights)),
            "size_bytes": weights.len(),
        }],
    });
    fs::write(
        root.join("vietdub-model.json"),
        serde_json::to_vec_pretty(&manifest).expect("serialize model manifest"),
    )
    .expect("model manifest");
}

#[test]
fn worker_manager_enforces_consent_on_client_creation() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let database = Database::in_memory().expect("database");
    let consents = ModelConsentRepository::new(database);
    let manager = WorkerManager::new(
        python_311(),
        repository_root().join("tests/fixtures/asr_workers"),
        consents.clone(),
    );
    let project_root = temporary.path().join("project");
    fs::create_dir(&project_root).expect("project root");
    let primary_root = temporary.path().join("primary-model");
    let fallback_root = temporary.path().join("fallback-model");
    let models = vec![
        RequiredModel {
            model_id: "funasr:paraformer-zh".into(),
            root: primary_root.clone(),
        },
        RequiredModel {
            model_id: "faster-whisper:large-v3".into(),
            root: fallback_root.clone(),
        },
    ];

    let error = manager
        .client_for_stage(StageName::Transcribe, &project_root, &models)
        .expect_err("missing consent blocks client");
    assert!(matches!(error, CoreError::ModelNotConsented));

    consents
        .insert_consent(&manifest(&models[0].model_id, "funasr"))
        .expect("primary consent");
    consents
        .insert_consent(&manifest(&models[1].model_id, "faster-whisper"))
        .expect("fallback consent");
    install_test_model(&primary_root, &models[0].model_id, "funasr");
    install_test_model(&fallback_root, &models[1].model_id, "faster-whisper");
    manager
        .client_for_stage(StageName::Transcribe, &project_root, &models)
        .expect("both consents allow client");
    fs::write(primary_root.join("weights.bin"), b"corrupt").expect("corrupt model fixture");
    let error = manager
        .client_for_stage(StageName::Transcribe, &project_root, &models)
        .expect_err("corrupt model is rejected");
    assert!(matches!(error, CoreError::ArtifactIntegrity));
}

#[tokio::test]
async fn asr_pipeline_registers_real_artifact_persists_segments_and_releases_review_slot() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let database = Database::in_memory().expect("database");
    let layout = ProjectLayout::new(temporary.path().join("projects")).expect("layout");
    let projects = ProjectService::new(ProjectRepository::new(database.clone()), layout.clone());
    let project = projects
        .create(&NewProject::chinese_to_vietnamese(
            "ASR Integration",
            WorkflowMode::Subtitles,
        ))
        .expect("project");
    let artifacts =
        ArtifactRegistry::new(ArtifactRepository::new(database.clone()), layout.clone());
    let audio_relative = format!("audio/original/{}.wav", Uuid::new_v4());
    let audio_path = layout
        .prepare_output(
            project.id,
            &vietdub_desktop_lib::infrastructure::ProjectRelativePath::parse(&audio_relative)
                .expect("audio relative path"),
        )
        .expect("audio path");
    fs::write(&audio_path, b"RIFF deterministic fixture").expect("audio fixture");
    let audio = artifacts
        .register_existing(
            project.id,
            ArtifactKind::OriginalAudio,
            &audio_relative,
            StageName::ExtractAudio,
            &Map::new(),
        )
        .expect("register audio");

    let consents = ModelConsentRepository::new(database.clone());
    let primary = "funasr:paraformer-zh";
    let fallback = "faster-whisper:large-v3";
    consents
        .insert_consent(&manifest(primary, "funasr"))
        .expect("primary consent");
    consents
        .insert_consent(&manifest(fallback, "faster-whisper"))
        .expect("fallback consent");
    let manager = WorkerManager::new(
        python_311(),
        repository_root().join("tests/fixtures/asr_workers"),
        consents,
    );
    let transcript = TranscriptService::new(
        SegmentRepository::new(database.clone()),
        InvalidationEngine::new(database.clone()),
    );
    let pipeline = AsrPipelineService::new(artifacts.clone(), layout, transcript.clone(), manager);
    let queue = PersistentQueue::new(database.clone(), 1).expect("queue");
    let stage = stage_run(project.id, StageName::Transcribe, StageScope::Project);
    let queued = queue.enqueue(&stage, 10).expect("enqueue ASR");
    let claimed = queue.claim_next().expect("claim").expect("claimed ASR job");
    assert_eq!(claimed.job.id, queued.id);
    let primary_path = temporary.path().join("models/funasr");
    let fallback_path = temporary.path().join("models/faster-whisper");
    install_test_model(&primary_path, primary, "funasr");
    install_test_model(&fallback_path, fallback, "faster-whisper");

    let result = pipeline
        .execute_claimed(
            &queue,
            claimed,
            &AsrExecutionRequest {
                audio_artifact_id: audio.id,
                primary_model_id: primary.into(),
                primary_model_path: primary_path.clone(),
                fallback_model_id: Some(fallback.into()),
                fallback_model_path: Some(fallback_path.clone()),
                language: "zh".into(),
                region: None,
            },
        )
        .await
        .expect("execute ASR pipeline");

    assert_eq!(queue.get(queued.id).unwrap().status, JobStatus::Completed);
    assert_eq!(queue.in_flight_count().unwrap(), 0);
    assert_eq!(result.segments.len(), 2);
    assert_eq!(
        artifacts.verify(result.transcript_artifact.id).unwrap(),
        ArtifactVerification::Verified
    );
    let review_stage = StageRunRepository::new(database.clone())
        .find_latest_by_status(
            project.id,
            StageName::TranscriptReview,
            StageStatus::ReviewRequired,
        )
        .expect("query review")
        .expect("review checkpoint");
    assert_eq!(review_stage.status, StageStatus::ReviewRequired);
    assert_eq!(
        queue.get(result.review_job_id).unwrap().status,
        JobStatus::Completed
    );
    transcript
        .approve_transcript(project.id)
        .expect("approve initial transcript");
    queue
        .complete_review(project.id)
        .expect("complete initial review");
    assert_eq!(
        StageRunRepository::new(database.clone())
            .get(review_stage.stage_id)
            .unwrap()
            .status,
        StageStatus::Completed
    );

    let preserved_second_id = result.segments[1].id;
    let regional_stage = stage_run(project.id, StageName::Transcribe, StageScope::Project);
    let regional_job = queue
        .enqueue(&regional_stage, 10)
        .expect("enqueue regional ASR");
    let regional_claim = queue
        .claim_next()
        .expect("claim regional")
        .expect("regional ASR job");
    let regional = pipeline
        .execute_claimed(
            &queue,
            regional_claim,
            &AsrExecutionRequest {
                audio_artifact_id: audio.id,
                primary_model_id: primary.into(),
                primary_model_path: primary_path,
                fallback_model_id: Some(fallback.into()),
                fallback_model_path: Some(fallback_path),
                language: "zh".into(),
                region: Some(AsrRegion {
                    start_ms: 0,
                    end_ms: 2_000,
                }),
            },
        )
        .await
        .expect("regional rerun");
    assert_eq!(
        queue.get(regional_job.id).unwrap().status,
        JobStatus::Completed
    );
    assert_eq!(regional.segments.len(), 2);
    assert_eq!(regional.segments[1].id, preserved_second_id);
    transcript
        .approve_transcript(project.id)
        .expect("approve regional transcript");
    queue
        .complete_review(project.id)
        .expect("complete regional review");
}

#[test]
fn no_voice_cloning_or_milestone_seven_composer_worker() {
    // Milestone 6 adds separation, but voice cloning and Milestone 7 remain out of scope.
    let _src_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let workers_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.parent())
        .map(|p| p.join("workers"))
        .expect("find workers dir");

    assert!(workers_dir.join("translation").exists());
    assert!(workers_dir.join("tts").exists());
    assert!(workers_dir.join("separation").exists());
    assert!(!workers_dir.join("composer").exists());
    // No voice cloning
    assert!(!workers_dir.join("voice_clone").exists());
    assert!(!workers_dir
        .join("asr/providers/deterministic_provider.py")
        .exists());
}
