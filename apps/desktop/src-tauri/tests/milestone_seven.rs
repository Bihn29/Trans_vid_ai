use std::{
    env,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use serde_json::Map;
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
use vietdub_desktop_lib::{
    domain::{
        Artifact, ArtifactKind, ArtifactVerification, AspectPreset, CacheDescriptor,
        ComposerConfig, CropRect, JobStatus, MediaMetadata, NewProject, NewSegment, NewStageRun,
        SegmentUpdate, StageName, StageScope, StageStatus, SubtitleMode, TextOverlay, WorkflowMode,
    },
    infrastructure::{
        build_render_plan, sha256_file, ArtifactRegistry, ComposerAssetService,
        ComposerExecutionRequest, ComposerExportService, ComposerPipelineService, ProjectLayout,
        ProjectService, TranscriptService,
    },
    jobs::{InvalidationChange, InvalidationEngine, PersistentQueue},
    media::FfprobeAdapter,
    persistence::{
        ArtifactRepository, ComposerRepository, Database, ProjectRepository, SegmentRepository,
        StageRunRepository,
    },
    processes::{ApprovedTool, ProcessLimits, SupervisedProcess},
};

struct Harness {
    temp: tempfile::TempDir,
    database: Database,
    layout: ProjectLayout,
    artifacts: ArtifactRegistry,
    project: Uuid,
    source: Artifact,
    audio: Artifact,
}

impl Harness {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let database = Database::in_memory().unwrap();
        let layout = ProjectLayout::new(temp.path().join("projects")).unwrap();
        let project = ProjectService::new(ProjectRepository::new(database.clone()), layout.clone())
            .create(&NewProject::chinese_to_vietnamese(
                "Composer",
                WorkflowMode::Dubbed,
            ))
            .unwrap()
            .id;
        let artifacts =
            ArtifactRegistry::new(ArtifactRepository::new(database.clone()), layout.clone());
        let root = layout.project_root(project).unwrap();
        fs::write(root.join("source/input.mp4"), b"immutable-source-video").unwrap();
        fs::write(root.join("audio/mixed/dub.wav"), b"mixed-audio").unwrap();
        let source = artifacts
            .register_existing(
                project,
                ArtifactKind::SourceVideo,
                "source/input.mp4",
                StageName::Import,
                &Map::new(),
            )
            .unwrap();
        let audio = artifacts
            .register_existing(
                project,
                ArtifactKind::MixedAudio,
                "audio/mixed/dub.wav",
                StageName::MixAudio,
                &Map::new(),
            )
            .unwrap();
        let transcript = TranscriptService::new(
            SegmentRepository::new(database.clone()),
            InvalidationEngine::new(database.clone()),
        );
        let segments = transcript
            .import_asr_results(
                project,
                vec![NewSegment {
                    id: Uuid::new_v4(),
                    project_id: project,
                    sequence: 0,
                    start_ms: 200,
                    end_ms: 1_800,
                    source_text: "你好".into(),
                    speaker_id: None,
                    asr_confidence: Some(0.99),
                }],
            )
            .unwrap();
        transcript
            .update_segment(
                project,
                segments[0].id,
                &SegmentUpdate {
                    translated_text: Some("Xin chào --> an toàn".into()),
                    ..Default::default()
                },
            )
            .unwrap();
        Self {
            temp,
            database,
            layout,
            artifacts,
            project,
            source,
            audio,
        }
    }

    fn request(&self) -> ComposerExecutionRequest {
        ComposerExecutionRequest {
            project_id: self.project,
            source_artifact_id: self.source.id,
            mixed_audio_artifact_id: self.audio.id,
            config: ComposerConfig::defaults(self.project),
        }
    }

    fn pipeline(&self, mode: &str) -> ComposerPipelineService {
        let limits = supervisor(Duration::from_secs(8));
        ComposerPipelineService::new(
            self.layout.clone(),
            self.artifacts.clone(),
            SegmentRepository::new(self.database.clone()),
            fake_tool(mode),
            FfprobeAdapter::new(fake_tool("probe"), limits.clone()),
            limits,
        )
    }
}

#[test]
fn plans_validate_bounds_aspects_and_keep_user_text_out_of_filters() {
    let project = Uuid::new_v4();
    let source = metadata();
    let mut config = ComposerConfig::defaults(project);
    config.subtitle_mode = SubtitleMode::None;
    config.text_overlays.push(TextOverlay {
        text: "x':,[]; movie=/outside".into(),
        x: 4,
        y: 5,
        font_size: 24,
        color: "#ffffff".into(),
        start_ms: 0,
        end_ms: 1_000,
    });
    for (aspect, expected) in [
        (AspectPreset::Source, (640, 360)),
        (AspectPreset::Landscape16x9, (1920, 1080)),
        (AspectPreset::Square1x1, (1080, 1080)),
        (AspectPreset::Vertical9x16, (1080, 1920)),
    ] {
        config.aspect = aspect;
        let plan = build_render_plan(
            &config,
            &source,
            "source/in.mp4",
            "audio/mixed/in.wav",
            None,
            &["metadata/text-safe.txt".into()],
            &[],
            "renders/out.mp4",
            "a".repeat(64),
        )
        .unwrap();
        assert_eq!((plan.output_width, plan.output_height), expected);
        assert!(!plan.filter_graph.contains("movie="));
        assert!(!plan.filter_graph.contains("x':,[]"));
        assert!(plan
            .filter_graph
            .contains("textfile='metadata/text-safe.txt'"));
    }
    config.crop = Some(CropRect {
        x: 630,
        y: 0,
        width: 20,
        height: 20,
    });
    assert!(config.validate_for_source(&source).is_err());
}

#[test]
fn soft_and_burned_subtitles_have_distinct_typed_plans() {
    let source = metadata();
    let mut config = ComposerConfig::defaults(Uuid::new_v4());
    let soft = build_render_plan(
        &config,
        &source,
        "source/in.mp4",
        "audio/mixed/in.wav",
        Some("subtitles/vi.srt"),
        &[],
        &[],
        "renders/out.mp4",
        "b".repeat(64),
    )
    .unwrap();
    assert!(soft
        .arguments
        .windows(2)
        .any(|pair| pair == ["-c:s", "mov_text"]));
    assert!(!soft.filter_graph.contains("subtitles="));
    config.subtitle_mode = SubtitleMode::Burned;
    let burned = build_render_plan(
        &config,
        &source,
        "source/in.mp4",
        "audio/mixed/in.wav",
        Some("subtitles/vi.srt"),
        &[],
        &[],
        "renders/out.mp4",
        "c".repeat(64),
    )
    .unwrap();
    assert!(burned
        .filter_graph
        .contains("subtitles=filename='subtitles/vi.srt'"));
    assert!(!burned
        .arguments
        .windows(2)
        .any(|pair| pair == ["-c:s", "mov_text"]));
}

#[tokio::test]
async fn render_registers_mp4_srt_qc_and_preserves_verified_source() {
    let harness = Harness::new();
    let before = fs::read(
        harness
            .layout
            .project_root(harness.project)
            .unwrap()
            .join(&harness.source.relative_path),
    )
    .unwrap();
    let result = harness
        .pipeline("ffmpeg")
        .execute(&harness.request(), CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(result.render.kind, ArtifactKind::Render);
    assert_eq!(
        result.subtitle.as_ref().unwrap().kind,
        ArtifactKind::Subtitle
    );
    assert!(result.quality_report.passes());
    assert_eq!(
        (result.quality_report.width, result.quality_report.height),
        (640, 360)
    );
    assert_eq!(
        harness.artifacts.verify(result.render.id).unwrap(),
        ArtifactVerification::Verified
    );
    assert_eq!(
        fs::read(
            harness
                .layout
                .project_root(harness.project)
                .unwrap()
                .join(&harness.source.relative_path)
        )
        .unwrap(),
        before
    );
    assert_eq!(
        harness.artifacts.verify(harness.source.id).unwrap(),
        ArtifactVerification::Verified
    );
}

#[test]
fn composer_config_persists_and_invalidates_render_dependencies() {
    let harness = Harness::new();
    let stages = StageRunRepository::new(harness.database.clone());
    let run = stages
        .insert(&stage(harness.project, StageName::Render))
        .unwrap();
    stages
        .set_status(run.stage_id, StageStatus::Completed, 100.0, None, None)
        .unwrap();
    let repository = ComposerRepository::new(harness.database.clone());
    let mut config = ComposerConfig::defaults(harness.project);
    config.speed = 1.25;
    assert_eq!(repository.save_config(&config).unwrap().speed, 1.25);
    let invalidated = InvalidationEngine::new(harness.database.clone())
        .invalidate(harness.project, &InvalidationChange::Composition)
        .unwrap();
    assert_eq!(invalidated, vec![run.stage_id]);
    assert_eq!(
        stages.get(run.stage_id).unwrap().status,
        StageStatus::Invalidated
    );
}

#[tokio::test]
async fn cancelled_render_is_isolated_and_can_be_retried() {
    let harness = Harness::new();
    let queue = PersistentQueue::new(harness.database.clone(), 1).unwrap();
    let job = queue
        .enqueue(&stage(harness.project, StageName::Render), 50)
        .unwrap();
    let claimed = queue.claim_next().unwrap().unwrap();
    let request = harness.request();
    let pipeline = harness.pipeline("sleep");
    let render = pipeline.execute_claimed(&queue, claimed, &request);
    let cancel = async {
        tokio::time::sleep(Duration::from_millis(150)).await;
        queue.cancel(job.id).unwrap();
    };
    let (outcome, ()) = tokio::join!(render, cancel);
    assert!(outcome.unwrap().is_none());
    assert_eq!(queue.get(job.id).unwrap().status, JobStatus::Cancelled);
    assert!(!harness
        .layout
        .project_root(harness.project)
        .unwrap()
        .join("renders")
        .read_dir()
        .unwrap()
        .any(|entry| entry
            .unwrap()
            .path()
            .extension()
            .is_some_and(|ext| ext == "mp4")));

    let retry = queue.retry(job.id).unwrap();
    let claimed_retry = queue.claim_next().unwrap().unwrap();
    assert_eq!(claimed_retry.job.id, retry.id);
    let result = harness
        .pipeline("ffmpeg")
        .execute_claimed(&queue, claimed_retry, &harness.request())
        .await
        .unwrap();
    assert!(result.is_some());
    assert_eq!(queue.get(retry.id).unwrap().status, JobStatus::Completed);
}

#[test]
fn export_allows_only_verified_srt_or_wav_and_never_overwrites() {
    let harness = Harness::new();
    let root = harness.layout.project_root(harness.project).unwrap();
    fs::write(
        root.join("subtitles/manual.srt"),
        b"1\n00:00:00,000 --> 00:00:01,000\nXin chao\n",
    )
    .unwrap();
    let subtitle = harness
        .artifacts
        .register_existing(
            harness.project,
            ArtifactKind::Subtitle,
            "subtitles/manual.srt",
            StageName::ComposeVideo,
            &Map::new(),
        )
        .unwrap();
    let export = ComposerExportService::new(harness.layout.clone(), harness.artifacts.clone());
    let target = harness.temp.path().join("export.srt");
    let exported = export
        .export(harness.project, subtitle.id, &target)
        .unwrap();
    assert_eq!(exported.file_name(), target.file_name());
    assert!(target.is_file());
    assert!(export
        .export(harness.project, subtitle.id, &target)
        .is_err());
    assert!(export
        .export(
            harness.project,
            harness.source.id,
            &harness.temp.path().join("bad.mp4")
        )
        .is_err());
    fs::write(root.join("subtitles/manual.srt"), b"corrupt").unwrap();
    assert!(export
        .export(
            harness.project,
            subtitle.id,
            &harness.temp.path().join("corrupt.srt")
        )
        .is_err());
}

#[test]
fn overlay_import_is_bounded_generated_and_registry_verified() {
    let harness = Harness::new();
    let source = harness.temp.path().join("logo & inert.png");
    fs::write(&source, b"bounded-image-fixture").unwrap();
    let importer =
        ComposerAssetService::new(harness.layout.clone(), harness.artifacts.clone(), 64).unwrap();
    let artifact = importer.import_overlay(harness.project, &source).unwrap();
    assert_eq!(artifact.project_id, harness.project);
    assert_eq!(artifact.kind, ArtifactKind::OverlayImage);
    assert!(artifact.relative_path.starts_with("metadata/overlay-"));
    assert!(!artifact.relative_path.contains("inert"));
    assert_eq!(
        harness.artifacts.verify(artifact.id).unwrap(),
        ArtifactVerification::Verified
    );

    let oversized = harness.temp.path().join("large.png");
    fs::write(&oversized, vec![0_u8; 65]).unwrap();
    assert!(importer
        .import_overlay(harness.project, &oversized)
        .is_err());
    let unsupported = harness.temp.path().join("logo.gif");
    fs::write(&unsupported, b"gif").unwrap();
    assert!(importer
        .import_overlay(harness.project, &unsupported)
        .is_err());
}

fn metadata() -> MediaMetadata {
    MediaMetadata {
        duration_ms: 2_500,
        width: 640,
        height: 360,
        frame_rate: 25.0,
        video_codec: "h264".into(),
        audio_codec: Some("aac".into()),
        container: "mp4".into(),
        rotation_degrees: 0,
    }
}

fn stage(project: Uuid, name: StageName) -> NewStageRun {
    NewStageRun::new(
        project,
        name,
        StageScope::Project,
        CacheDescriptor {
            schema_version: 1,
            input_hash: format!("{:x}", Sha256::digest(b"composer-input")),
            config_hash: format!("{:x}", Sha256::digest(b"composer-config")),
            engine_name: "composer".into(),
            engine_version: "1".into(),
            model_version: "none".into(),
            metadata: Map::new(),
        },
        "composer",
    )
}

fn supervisor(timeout: Duration) -> SupervisedProcess {
    SupervisedProcess::new(ProcessLimits {
        timeout,
        max_stdout_bytes: 64 * 1024,
        max_stderr_bytes: 4096,
    })
    .unwrap()
}

fn fake_tool(mode: &str) -> ApprovedTool {
    let python = env::var("VIETDUB_PYTHON")
        .map(PathBuf::from)
        .unwrap_or_else(|_| workspace_root().join(".venv/Scripts/python.exe"));
    ApprovedTool::with_fixed_args(
        &python,
        sha256_file(&python).unwrap().0,
        [
            workspace_root()
                .join("tests/fixtures/tools/fake_media_tool.py")
                .into_os_string(),
            OsString::from(mode),
        ],
    )
    .unwrap()
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .unwrap()
        .to_path_buf()
}
