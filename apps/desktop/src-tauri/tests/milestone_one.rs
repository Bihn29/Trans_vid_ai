use std::fs;

use serde_json::Map;
use tempfile::TempDir;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
use vietdub_desktop_lib::{
    domain::{
        ArtifactKind, ArtifactVerification, CacheDescriptor, CoreError, Job, JobStatus, NewProject,
        NewStageRun, Project, ProjectStatus, ProjectUpdate, StageName, StageScope, StageStatus,
        WorkflowMode,
    },
    infrastructure::{ArtifactRegistry, ProjectLayout, ProjectRelativePath, ProjectService},
    jobs::{
        CacheResolver, InvalidationChange, InvalidationEngine, PersistentQueue, ProviderOutcome,
        StageProvider,
    },
    persistence::{
        ArtifactRepository, Database, JobRepository, ProjectRepository, StageRunRepository,
    },
};

struct Harness {
    _temporary: TempDir,
    database: Database,
    layout: ProjectLayout,
    projects: ProjectService,
    artifacts: ArtifactRegistry,
    stages: StageRunRepository,
    jobs: JobRepository,
}

impl Harness {
    fn new() -> Self {
        let temporary = tempfile::tempdir().expect("temporary test root");
        let database = Database::in_memory().expect("in-memory database");
        let layout = ProjectLayout::new(temporary.path().join("projects")).expect("project layout");
        let projects =
            ProjectService::new(ProjectRepository::new(database.clone()), layout.clone());
        let artifacts =
            ArtifactRegistry::new(ArtifactRepository::new(database.clone()), layout.clone());
        Self {
            _temporary: temporary,
            database: database.clone(),
            layout,
            projects,
            artifacts,
            stages: StageRunRepository::new(database.clone()),
            jobs: JobRepository::new(database),
        }
    }

    fn create_project(&self, name: &str) -> Project {
        self.projects
            .create(&NewProject::chinese_to_vietnamese(
                name,
                WorkflowMode::Dubbed,
            ))
            .expect("create project")
    }

    fn stage(
        &self,
        project_id: Uuid,
        stage: StageName,
        scope: StageScope,
        seed: u64,
    ) -> NewStageRun {
        let cache = CacheDescriptor {
            schema_version: 1,
            input_hash: format!("{seed:064x}"),
            config_hash: format!("{:064x}", seed + 1),
            engine_name: "deterministic-test".into(),
            engine_version: "1".into(),
            model_version: "none".into(),
            metadata: Map::new(),
        };
        NewStageRun::new(project_id, stage, scope, cache, "deterministic-test")
    }

    fn completed_stage(
        &self,
        project_id: Uuid,
        stage: StageName,
        scope: StageScope,
        seed: u64,
    ) -> Uuid {
        let run = self
            .stages
            .insert(&self.stage(project_id, stage, scope, seed))
            .expect("insert stage");
        self.stages
            .set_status(run.stage_id, StageStatus::Completed, 100.0, None, None)
            .expect("complete stage");
        run.stage_id
    }
}

#[test]
fn project_crud_creates_layout_snapshot_and_recoverable_delete() {
    let harness = Harness::new();
    let created = harness.create_project("Dự án kiểm thử");
    let project_root = harness
        .layout
        .project_root(created.id)
        .expect("project root");
    let snapshot: serde_json::Value = serde_json::from_slice(
        &fs::read(project_root.join("project.json")).expect("read project snapshot"),
    )
    .expect("parse snapshot");
    assert_eq!(snapshot["schema_version"], 1);
    assert_eq!(snapshot["name"], "Dự án kiểm thử");

    let updated = harness
        .projects
        .update(
            created.id,
            &ProjectUpdate {
                name: Some("Tên mới".into()),
                status: Some(ProjectStatus::Active),
                ..ProjectUpdate::default()
            },
        )
        .expect("update project");
    assert_eq!(updated.name, "Tên mới");
    assert_eq!(harness.projects.list().expect("list projects").len(), 1);

    harness.projects.delete(created.id).expect("delete project");
    assert!(matches!(
        harness.projects.get(created.id),
        Err(CoreError::NotFound("project"))
    ));
    assert!(!harness.layout.root().join(created.id.to_string()).exists());
    assert_eq!(
        fs::read_dir(harness.layout.root().join(".trash"))
            .expect("read trash")
            .count(),
        1
    );
}

#[test]
fn project_paths_reject_traversal_and_symlink_escape() {
    let harness = Harness::new();
    let project = harness.create_project("Path security");
    for path in [
        "../outside",
        "metadata/../../outside",
        "C:/outside",
        "NUL.txt",
    ] {
        assert!(ProjectRelativePath::parse(path).is_err(), "{path}");
    }

    let outside = harness.layout.root().join("outside");
    fs::create_dir(&outside).expect("outside directory");
    fs::write(outside.join("secret.txt"), b"secret").expect("outside fixture");
    let link = harness
        .layout
        .project_root(project.id)
        .expect("project root")
        .join("metadata")
        .join("outside-link");
    if create_directory_symlink(&outside, &link) {
        let relative = ProjectRelativePath::parse("metadata/outside-link/secret.txt")
            .expect("syntactically valid path");
        assert!(matches!(
            harness.layout.resolve_existing(project.id, &relative),
            Err(CoreError::UnsafePath)
        ));
    }
}

#[test]
fn artifact_registry_detects_mutation_and_missing_file() {
    let harness = Harness::new();
    let project = harness.create_project("Artifact integrity");
    let relative = ProjectRelativePath::parse("metadata/result.json").expect("relative path");
    let path = harness
        .layout
        .prepare_output(project.id, &relative)
        .expect("output path");
    fs::write(&path, br#"{"ok":true}"#).expect("write artifact");

    let artifact = harness
        .artifacts
        .register_existing(
            project.id,
            ArtifactKind::Metadata,
            relative.as_str(),
            StageName::Import,
            &Map::new(),
        )
        .expect("register artifact");
    assert_eq!(
        harness
            .artifacts
            .verify(artifact.id)
            .expect("verify artifact"),
        ArtifactVerification::Verified
    );

    fs::write(&path, br#"{"ok":false}"#).expect("mutate artifact");
    assert_eq!(
        harness
            .artifacts
            .verify(artifact.id)
            .expect("detect corruption"),
        ArtifactVerification::Corrupt
    );
    fs::remove_file(path).expect("remove artifact");
    assert_eq!(
        harness
            .artifacts
            .verify(artifact.id)
            .expect("detect missing"),
        ArtifactVerification::Missing
    );
}

#[test]
fn cache_reuse_requires_identity_scope_and_verified_artifacts() {
    let harness = Harness::new();
    let project = harness.create_project("Cache integrity");
    let segment_id = Uuid::new_v4();
    let requested = harness.stage(
        project.id,
        StageName::Synthesize,
        StageScope::Segment(segment_id),
        60,
    );
    let completed = harness
        .stages
        .insert(&requested)
        .expect("insert cached stage");

    let relative = ProjectRelativePath::parse("audio/tts/cache.wav").expect("cache path");
    let path = harness
        .layout
        .prepare_output(project.id, &relative)
        .expect("cache output path");
    fs::write(&path, b"deterministic-audio").expect("cache fixture");
    let artifact = harness
        .artifacts
        .register_existing(
            project.id,
            ArtifactKind::Tts,
            relative.as_str(),
            StageName::Synthesize,
            &Map::new(),
        )
        .expect("register cached artifact");
    harness
        .stages
        .set_outputs(completed.stage_id, &[artifact.id])
        .expect("attach cached artifact");
    harness
        .stages
        .set_status(
            completed.stage_id,
            StageStatus::Completed,
            100.0,
            None,
            None,
        )
        .expect("complete cached stage");

    let resolver = CacheResolver::new(harness.stages.clone(), harness.artifacts.clone());
    assert_eq!(
        resolver
            .reusable(&requested)
            .expect("resolve valid cache")
            .expect("cache hit")
            .stage_id,
        completed.stage_id
    );
    let other_scope = harness.stage(
        project.id,
        StageName::Synthesize,
        StageScope::Segment(Uuid::new_v4()),
        60,
    );
    assert!(resolver
        .reusable(&other_scope)
        .expect("resolve other scope")
        .is_none());

    fs::write(path, b"corrupted-audio").expect("corrupt cache fixture");
    assert!(resolver
        .reusable(&requested)
        .expect("reject corrupt cache")
        .is_none());
    assert_eq!(
        harness
            .stages
            .get(completed.stage_id)
            .expect("invalidated cached stage")
            .status,
        StageStatus::Invalidated
    );
}

#[test]
fn invalidation_is_targeted_to_changed_segment_and_project_tail() {
    let harness = Harness::new();
    let project = harness.create_project("Invalidation");
    let first_segment = Uuid::new_v4();
    let second_segment = Uuid::new_v4();
    let translate_first = harness.completed_stage(
        project.id,
        StageName::Translate,
        StageScope::Segment(first_segment),
        1,
    );
    let synthesize_first = harness.completed_stage(
        project.id,
        StageName::Synthesize,
        StageScope::Segment(first_segment),
        2,
    );
    let translate_second = harness.completed_stage(
        project.id,
        StageName::Translate,
        StageScope::Segment(second_segment),
        3,
    );
    let transcribe =
        harness.completed_stage(project.id, StageName::Transcribe, StageScope::Project, 4);
    let mix = harness.completed_stage(project.id, StageName::MixAudio, StageScope::Project, 5);

    let affected = InvalidationEngine::new(harness.database.clone())
        .invalidate(
            project.id,
            &InvalidationChange::SourceTranscript {
                segment_id: first_segment,
            },
        )
        .expect("invalidate dependent stages");

    assert!(affected.contains(&translate_first));
    assert!(affected.contains(&synthesize_first));
    assert!(affected.contains(&mix));
    assert!(!affected.contains(&translate_second));
    assert!(!affected.contains(&transcribe));
    assert_eq!(
        harness
            .stages
            .get(translate_second)
            .expect("second translation")
            .status,
        StageStatus::Completed
    );
    assert_eq!(
        harness.stages.get(transcribe).expect("transcribe").status,
        StageStatus::Completed
    );
}

#[test]
fn invalidation_refuses_to_race_a_running_stage() {
    let harness = Harness::new();
    let project = harness.create_project("Invalidation race");
    let run = harness
        .stages
        .insert(&harness.stage(project.id, StageName::Render, StageScope::Project, 10))
        .expect("insert render");
    harness
        .stages
        .set_status(run.stage_id, StageStatus::Running, 20.0, None, None)
        .expect("run render");

    let error = InvalidationEngine::new(harness.database.clone())
        .invalidate(project.id, &InvalidationChange::Composition)
        .expect_err("running stage blocks invalidation");
    assert!(matches!(error, CoreError::RunningStageConflict));
    assert_eq!(
        harness
            .stages
            .get(run.stage_id)
            .expect("render stage")
            .status,
        StageStatus::Running
    );
}

#[test]
fn queue_honors_priority_concurrency_and_cancellation_isolation() {
    let harness = Harness::new();
    let first_project = harness.create_project("First queue project");
    let second_project = harness.create_project("Second queue project");
    let queue = PersistentQueue::new(harness.database.clone(), 2).expect("queue");
    queue.set_concurrency(1).expect("limit concurrency");
    let low = queue
        .enqueue(
            &harness.stage(first_project.id, StageName::Import, StageScope::Project, 20),
            1,
        )
        .expect("low priority job");
    let high = queue
        .enqueue(
            &harness.stage(
                second_project.id,
                StageName::Import,
                StageScope::Project,
                21,
            ),
            10,
        )
        .expect("high priority job");

    let high_claim = queue.claim_next().expect("claim high").expect("high job");
    assert_eq!(high_claim.job.id, high.id);
    assert!(queue.claim_next().expect("configured limit").is_none());
    queue.set_concurrency(2).expect("raise concurrency");
    let low_claim = queue.claim_next().expect("claim low").expect("low job");
    assert_eq!(low_claim.job.id, low.id);
    assert!(queue.claim_next().expect("concurrency full").is_none());

    queue.cancel(high.id).expect("request high cancellation");
    assert!(high_claim.cancellation.is_cancelled());
    assert!(!low_claim.cancellation.is_cancelled());
    assert_eq!(
        queue
            .acknowledge_interruption(high.id)
            .expect("acknowledge cancellation")
            .status,
        JobStatus::Cancelled
    );
    assert_eq!(
        queue
            .complete(low.id, &[])
            .expect("complete low job")
            .status,
        JobStatus::Completed
    );
    assert_eq!(queue.in_flight_count().expect("in-flight count"), 0);
}

#[test]
fn queue_pause_resume_and_retry_preserve_history() {
    let harness = Harness::new();
    let project = harness.create_project("Queue transitions");
    let queue = PersistentQueue::new(harness.database.clone(), 1).expect("queue");
    let paused = queue
        .enqueue(
            &harness.stage(project.id, StageName::Probe, StageScope::Project, 30),
            0,
        )
        .expect("queued job");
    assert_eq!(
        queue.pause(paused.id).expect("pause queued").status,
        JobStatus::Paused
    );
    assert!(queue.claim_next().expect("paused not claimable").is_none());
    assert_eq!(
        queue.resume(paused.id).expect("resume").status,
        JobStatus::Queued
    );

    let claimed = queue.claim_next().expect("claim").expect("resumed job");
    queue
        .fail(
            claimed.job.id,
            "DETERMINISTIC_FAILURE",
            "Tác vụ kiểm thử thất bại.",
        )
        .expect("fail job");
    let retry = queue.retry(claimed.job.id).expect("retry failed job");

    assert_eq!(retry.attempt, 2);
    assert_eq!(retry.retry_of_job_id, Some(claimed.job.id));
    assert_ne!(retry.stage_run_id, claimed.job.stage_run_id);
    assert_eq!(
        queue.get(claimed.job.id).expect("original job").status,
        JobStatus::Failed
    );
    assert_eq!(
        harness
            .stages
            .get(claimed.job.stage_run_id)
            .expect("original stage")
            .status,
        StageStatus::Failed
    );
}

#[test]
fn restart_recovery_respects_pause_cancel_and_requeues_other_work() {
    let harness = Harness::new();
    let project = harness.create_project("Recovery");
    let original_queue = PersistentQueue::new(harness.database.clone(), 3).expect("queue");
    let mut jobs = Vec::new();
    for (seed, stage) in [
        (40, StageName::Import),
        (41, StageName::Probe),
        (42, StageName::Normalize),
    ] {
        jobs.push(
            original_queue
                .enqueue(
                    &harness.stage(project.id, stage, StageScope::Project, seed),
                    0,
                )
                .expect("enqueue recovery job"),
        );
    }
    let first = original_queue
        .claim_next()
        .expect("claim first")
        .expect("first");
    let second = original_queue
        .claim_next()
        .expect("claim second")
        .expect("second");
    let third = original_queue
        .claim_next()
        .expect("claim third")
        .expect("third");
    original_queue.pause(first.job.id).expect("request pause");
    original_queue
        .cancel(second.job.id)
        .expect("request cancel");

    let recovered_queue = PersistentQueue::new(harness.database.clone(), 3).expect("new queue");
    let report = recovered_queue.recover_interrupted().expect("recover jobs");
    assert_eq!(report.paused, 1);
    assert_eq!(report.cancelled, 1);
    assert_eq!(report.requeued, 1);
    assert_eq!(
        recovered_queue
            .get(first.job.id)
            .expect("paused job")
            .status,
        JobStatus::Paused
    );
    assert_eq!(
        recovered_queue
            .get(second.job.id)
            .expect("cancelled job")
            .status,
        JobStatus::Cancelled
    );
    assert_eq!(
        recovered_queue
            .get(third.job.id)
            .expect("requeued job")
            .status,
        JobStatus::Queued
    );
    assert_eq!(
        harness
            .jobs
            .list_for_project(project.id)
            .expect("job history")
            .len(),
        jobs.len()
    );
}

#[test]
fn restart_recovery_survives_sqlite_close_and_reopen() {
    let temporary = tempfile::tempdir().expect("temporary restart root");
    let database_path = temporary.path().join("state").join("vietdub.db");
    let projects_root = temporary.path().join("projects");

    let (project_id, job_id, stage_id) = {
        let database = Database::open(&database_path).expect("open first database");
        let layout = ProjectLayout::new(projects_root.clone()).expect("first project layout");
        let projects = ProjectService::new(ProjectRepository::new(database.clone()), layout);
        let project = projects
            .create(&NewProject::chinese_to_vietnamese(
                "Persistent recovery",
                WorkflowMode::Dubbed,
            ))
            .expect("create persistent project");
        let queue = PersistentQueue::new(database.clone(), 1).expect("first queue");
        let stage = NewStageRun::new(
            project.id,
            StageName::Import,
            StageScope::Project,
            CacheDescriptor {
                schema_version: 1,
                input_hash: "a".repeat(64),
                config_hash: "b".repeat(64),
                engine_name: "deterministic-test".into(),
                engine_version: "1".into(),
                model_version: "none".into(),
                metadata: Map::new(),
            },
            "deterministic-test",
        );
        let job = queue.enqueue(&stage, 7).expect("enqueue persistent job");
        let claimed = queue
            .claim_next()
            .expect("claim persistent job")
            .expect("persistent job");
        assert_eq!(claimed.job.status, JobStatus::Running);
        (project.id, job.id, job.stage_run_id)
    };

    let reopened = Database::open(&database_path).expect("reopen database");
    assert!(ProjectRepository::new(reopened.clone())
        .get(project_id)
        .is_ok());
    let queue = PersistentQueue::new(reopened.clone(), 1).expect("reopened queue");
    let report = queue.recover_interrupted().expect("recover after reopen");
    assert_eq!(report.requeued, 1);
    assert_eq!(
        queue.get(job_id).expect("recovered job").status,
        JobStatus::Queued
    );
    assert_eq!(
        StageRunRepository::new(reopened)
            .get(stage_id)
            .expect("recovered stage")
            .status,
        StageStatus::Queued
    );
    assert_eq!(
        queue
            .claim_next()
            .expect("claim after reopen")
            .expect("recovered claim")
            .job
            .id,
        job_id
    );
}

#[test]
fn review_checkpoint_releases_the_queue_slot() {
    let harness = Harness::new();
    let project = harness.create_project("Review checkpoint");
    let queue = PersistentQueue::new(harness.database.clone(), 1).expect("queue");
    let review_job = queue
        .enqueue(
            &harness.stage(
                project.id,
                StageName::TranscriptReview,
                StageScope::Project,
                50,
            ),
            5,
        )
        .expect("review job");
    queue
        .enqueue(
            &harness.stage(project.id, StageName::Translate, StageScope::Project, 51),
            0,
        )
        .expect("next job");

    let claimed = queue
        .claim_next()
        .expect("claim review")
        .expect("review job");
    let completed = queue
        .execute(
            claimed,
            &DeterministicProvider {
                outcome: ProviderOutcome::ReviewRequired,
            },
        )
        .expect("enter review checkpoint");
    assert_eq!(completed.id, review_job.id);
    assert_eq!(completed.status, JobStatus::Completed);
    assert_eq!(queue.in_flight_count().expect("released slot"), 0);
    assert_eq!(
        harness
            .stages
            .get(review_job.stage_run_id)
            .expect("review stage")
            .status,
        StageStatus::ReviewRequired
    );
    assert!(queue
        .claim_next()
        .expect("claim next after review")
        .is_some());
}

struct DeterministicProvider {
    outcome: ProviderOutcome,
}

impl StageProvider for DeterministicProvider {
    fn execute(
        &self,
        _job: &Job,
        _cancellation: &CancellationToken,
    ) -> Result<ProviderOutcome, CoreError> {
        Ok(self.outcome.clone())
    }
}

#[cfg(windows)]
fn create_directory_symlink(target: &std::path::Path, link: &std::path::Path) -> bool {
    match std::os::windows::fs::symlink_dir(target, link) {
        Ok(()) => true,
        Err(error) if error.raw_os_error() == Some(1314) => false,
        Err(error) => panic!("create directory symlink: {error}"),
    }
}

#[cfg(unix)]
fn create_directory_symlink(target: &std::path::Path, link: &std::path::Path) -> bool {
    std::os::unix::fs::symlink(target, link).expect("create directory symlink");
    true
}
