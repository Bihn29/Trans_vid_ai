use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::{env, fs, path::PathBuf};
use uuid::Uuid;
use vietdub_desktop_lib::{
    domain::{
        Artifact, ArtifactKind, ArtifactVerification, AudioMixSettings, CacheDescriptor, JobStatus,
        NewProject, NewSegment, NewStageRun, NewTtsSegmentRun, SegmentUpdate,
        SeparationEngineDescriptor, StageName, StageScope, WorkflowMode,
    },
    infrastructure::{
        ArtifactRegistry, AudioMixRequest, AudioPipelineService, ProjectLayout, ProjectService,
        SeparationExecutionRequest, TranscriptService,
    },
    jobs::{InvalidationEngine, PersistentQueue},
    persistence::{
        ArtifactRepository, AudioRepository, Database, ModelConsentRepository, ProjectRepository,
        SegmentRepository, TtsRepository,
    },
    workers::WorkerManager,
};

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .unwrap()
        .to_path_buf()
}

fn python() -> PathBuf {
    env::var("VIETDUB_PYTHON")
        .map(PathBuf::from)
        .unwrap_or_else(|_| repository_root().join(".venv/Scripts/python.exe"))
}

fn wav(samples: &[i16], rate: u32) -> Vec<u8> {
    let size = (samples.len() * 2) as u32;
    let mut bytes = Vec::with_capacity(size as usize + 44);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + size).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&rate.to_le_bytes());
    bytes.extend_from_slice(&(rate * 2).to_le_bytes());
    bytes.extend_from_slice(&2_u16.to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&size.to_le_bytes());
    for sample in samples {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    bytes
}

fn stage(project: Uuid, name: StageName) -> NewStageRun {
    let cache = CacheDescriptor {
        schema_version: 1,
        input_hash: format!("{:x}", Sha256::digest(format!("{name:?}-input"))),
        config_hash: format!("{:x}", Sha256::digest(format!("{name:?}-config"))),
        engine_name: "audio-test".into(),
        engine_version: "1".into(),
        model_version: "none".into(),
        metadata: Map::new(),
    };
    NewStageRun::new(project, name, StageScope::Project, cache, "audio-test")
}

fn engine() -> SeparationEngineDescriptor {
    SeparationEngineDescriptor {
        engine_id: "energy-mask-v1".into(),
        display_name: "VietDub Energy Mask".into(),
        version: "1.0.0".into(),
        license: "UNLICENSED".into(),
        install_mode: "bundled_source".into(),
        requires_consent: false,
        sends_data_off_device: false,
        approved: true,
    }
}

struct Harness {
    _temp: tempfile::TempDir,
    layout: ProjectLayout,
    artifacts: ArtifactRegistry,
    audio_repository: AudioRepository,
    pipeline: AudioPipelineService,
    queue: PersistentQueue,
    project: Uuid,
    source: Artifact,
    tts_artifacts: Vec<Artifact>,
}

fn setup(failing_separation: bool) -> Harness {
    let temp = tempfile::tempdir().unwrap();
    let database = Database::in_memory().unwrap();
    let layout = ProjectLayout::new(temp.path().join("projects")).unwrap();
    let project = ProjectService::new(ProjectRepository::new(database.clone()), layout.clone())
        .create(&NewProject::chinese_to_vietnamese(
            "Audio",
            WorkflowMode::Dubbed,
        ))
        .unwrap();
    let artifacts =
        ArtifactRegistry::new(ArtifactRepository::new(database.clone()), layout.clone());
    let source_samples = (0..32_000)
        .map(|index| if index < 16_000 { 1_000 } else { 8_000 })
        .collect::<Vec<i16>>();
    let source_relative = "audio/original/source.wav";
    fs::write(
        layout
            .project_root(project.id)
            .unwrap()
            .join(source_relative),
        wav(&source_samples, 16_000),
    )
    .unwrap();
    let source = artifacts
        .register_existing(
            project.id,
            ArtifactKind::OriginalAudio,
            source_relative,
            StageName::ExtractAudio,
            &Map::new(),
        )
        .unwrap();

    let transcript = TranscriptService::new(
        SegmentRepository::new(database.clone()),
        InvalidationEngine::new(database.clone()),
    );
    let segments = transcript
        .import_asr_results(
            project.id,
            vec![
                NewSegment {
                    id: Uuid::new_v4(),
                    project_id: project.id,
                    sequence: 0,
                    start_ms: 250,
                    end_ms: 750,
                    source_text: "one".into(),
                    speaker_id: None,
                    asr_confidence: Some(0.9),
                },
                NewSegment {
                    id: Uuid::new_v4(),
                    project_id: project.id,
                    sequence: 1,
                    start_ms: 1_000,
                    end_ms: 1_500,
                    source_text: "two".into(),
                    speaker_id: None,
                    asr_confidence: Some(0.9),
                },
            ],
        )
        .unwrap();
    for segment in &segments {
        transcript
            .update_segment(
                project.id,
                segment.id,
                &SegmentUpdate {
                    translated_text: Some("dub".into()),
                    ..Default::default()
                },
            )
            .unwrap();
    }

    let queue = PersistentQueue::new(database.clone(), 2).unwrap();
    let synth_job = queue
        .enqueue(&stage(project.id, StageName::Synthesize), 100)
        .unwrap();
    let claimed_synth = queue.claim_next().unwrap().unwrap();
    assert_eq!(claimed_synth.job.id, synth_job.id);
    let tts_repository = TtsRepository::new(database.clone());
    let mut tts_artifacts = Vec::new();
    let mut definitions = Vec::new();
    for segment in &segments {
        let relative = format!("audio/tts/{}.wav", segment.id);
        fs::write(
            layout.project_root(project.id).unwrap().join(&relative),
            // Local voice providers may emit 44.1 kHz while normalized source
            // audio is 16 kHz. The mixer must fit both to the transcript slot.
            wav(&vec![29_000; 11_025], 44_100),
        )
        .unwrap();
        let artifact = artifacts
            .register_existing(
                project.id,
                ArtifactKind::Tts,
                &relative,
                StageName::Synthesize,
                &Map::new(),
            )
            .unwrap();
        definitions.push(NewTtsSegmentRun {
            id: Uuid::new_v4(),
            project_id: project.id,
            stage_run_id: synth_job.stage_run_id,
            segment_id: segment.id,
            cache_identity: format!("{:x}", Sha256::digest(segment.id.as_bytes())),
            provider_id: "test".into(),
            voice_id: "test".into(),
            target_duration_ms: 500,
        });
        tts_artifacts.push(artifact);
    }
    let runs = tts_repository.insert_runs(&definitions).unwrap();
    for (run, artifact) in runs.iter().zip(&tts_artifacts) {
        tts_repository.mark_running(run.id).unwrap();
        tts_repository
            .complete(run.id, artifact.id, 250, 0.85, None)
            .unwrap();
    }
    queue
        .complete(
            synth_job.id,
            &tts_artifacts
                .iter()
                .map(|artifact| artifact.id)
                .collect::<Vec<_>>(),
        )
        .unwrap();

    let worker_root = if failing_separation {
        repository_root().join("tests/fixtures/separation_workers")
    } else {
        repository_root().join("workers")
    };
    let workers = WorkerManager::new(
        python(),
        worker_root,
        ModelConsentRepository::new(database.clone()),
    );
    let pipeline =
        AudioPipelineService::new(artifacts.clone(), layout.clone(), transcript, workers);
    Harness {
        _temp: temp,
        layout,
        artifacts,
        audio_repository: AudioRepository::new(database),
        pipeline,
        queue,
        project: project.id,
        source,
        tts_artifacts,
    }
}

async fn separate(
    harness: &Harness,
) -> vietdub_desktop_lib::infrastructure::SeparationExecutionResult {
    harness
        .queue
        .enqueue(&stage(harness.project, StageName::SeparateAudio), 50)
        .unwrap();
    harness
        .pipeline
        .execute_separation_claimed(
            &harness.queue,
            harness.queue.claim_next().unwrap().unwrap(),
            &SeparationExecutionRequest {
                source_artifact_id: harness.source.id,
                engine: engine(),
                energy_threshold: 0.5,
            },
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn approved_local_engine_separates_aligned_verified_stems() {
    let harness = setup(false);
    let result = separate(&harness).await;
    assert_eq!(result.mode, "separated");
    assert_eq!(result.vocals.kind, ArtifactKind::Vocals);
    assert_eq!(result.background.kind, ArtifactKind::Background);
    assert_eq!(
        result.vocals.metadata.get("duration_ms"),
        result.background.metadata.get("duration_ms")
    );
    assert_eq!(
        harness.artifacts.verify(result.vocals.id).unwrap(),
        ArtifactVerification::Verified
    );
}

#[tokio::test]
async fn provider_failure_uses_explicit_attenuation_fallback() {
    let harness = setup(true);
    let result = separate(&harness).await;
    assert_eq!(result.mode, "fallback_attenuation");
    assert_eq!(
        result.background.metadata.get("separation_mode"),
        Some(&Value::String("fallback_attenuation".into()))
    );
}

#[tokio::test]
async fn deterministic_mix_aligns_timeline_limits_clipping_and_preserves_tts() {
    let harness = setup(true);
    let separation = separate(&harness).await;
    let original_hashes = harness
        .tts_artifacts
        .iter()
        .map(|artifact| artifact.sha256.clone())
        .collect::<Vec<_>>();
    let mut settings = AudioMixSettings::defaults(harness.project);
    settings.voice_gain = 2.0;
    settings.target_rms_dbfs = -6.0;
    settings.limiter_peak = 0.2;
    harness.audio_repository.save_settings(&settings).unwrap();

    let mut hashes = Vec::new();
    let mut timeline_hashes = Vec::new();
    for _ in 0..2 {
        harness
            .queue
            .enqueue(&stage(harness.project, StageName::MixAudio), 40)
            .unwrap();
        let result = harness
            .pipeline
            .execute_mix_claimed(
                &harness.queue,
                harness.queue.claim_next().unwrap().unwrap(),
                &AudioMixRequest {
                    background_artifact_id: separation.background.id,
                    original_voice_artifact_id: Some(separation.vocals.id),
                    music_artifact_id: None,
                    settings: settings.clone(),
                },
            )
            .unwrap();
        assert!(result.quality.passes());
        assert_eq!(result.quality.duration_ms, 2_000);
        assert_eq!(result.quality.clipped_samples, 0);
        assert!(result.quality.limited_samples > 0);
        hashes.push(result.artifact.sha256);
        timeline_hashes.push(result.quality.timeline_hash);
    }
    assert_eq!(hashes[0], hashes[1]);
    assert_eq!(timeline_hashes[0], timeline_hashes[1]);
    for (artifact, hash) in harness.tts_artifacts.iter().zip(original_hashes) {
        assert_eq!(artifact.sha256, hash);
        assert_eq!(
            harness.artifacts.verify(artifact.id).unwrap(),
            ArtifactVerification::Verified
        );
    }
}

#[test]
fn settings_are_validated_persisted_and_project_scoped() {
    let harness = setup(true);
    let mut settings = AudioMixSettings::defaults(harness.project);
    settings.music_gain = 1.25;
    assert_eq!(
        harness
            .audio_repository
            .save_settings(&settings)
            .unwrap()
            .music_gain,
        1.25
    );
    settings.limiter_peak = 1.5;
    assert!(harness.audio_repository.save_settings(&settings).is_err());
    let other = AudioMixSettings::defaults(Uuid::new_v4());
    assert!(harness.audio_repository.save_settings(&other).is_err());
}

#[test]
fn project_layout_contains_music_directory_without_composer_outputs() {
    let harness = setup(true);
    let root = harness.layout.project_root(harness.project).unwrap();
    assert!(root.join("audio/music").is_dir());
    assert!(!repository_root().join("workers/composer").exists());
}

#[tokio::test]
async fn cancelling_mix_keeps_tts_artifacts_and_writes_no_mixed_output() {
    let harness = setup(true);
    let separation = separate(&harness).await;
    let job = harness
        .queue
        .enqueue(&stage(harness.project, StageName::MixAudio), 40)
        .unwrap();
    let claimed = harness.queue.claim_next().unwrap().unwrap();
    harness.queue.cancel(job.id).unwrap();
    assert!(harness
        .pipeline
        .execute_mix_claimed(
            &harness.queue,
            claimed,
            &AudioMixRequest {
                background_artifact_id: separation.background.id,
                original_voice_artifact_id: None,
                music_artifact_id: None,
                settings: AudioMixSettings::defaults(harness.project),
            },
        )
        .is_err());
    assert_eq!(
        harness.queue.get(job.id).unwrap().status,
        JobStatus::Cancelled
    );
    assert!(!harness
        .artifacts
        .list_for_project(harness.project)
        .unwrap()
        .iter()
        .any(|artifact| artifact.kind == ArtifactKind::MixedAudio));
    assert!(harness.tts_artifacts.iter().all(|artifact| {
        harness.artifacts.verify(artifact.id).unwrap() == ArtifactVerification::Verified
    }));
}
