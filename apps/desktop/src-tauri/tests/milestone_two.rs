use std::{
    ffi::OsString,
    fs,
    future::Future,
    net::IpAddr,
    path::{Path, PathBuf},
    pin::Pin,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

use tempfile::TempDir;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
use vietdub_desktop_lib::{
    domain::{ArtifactKind, ArtifactVerification, CoreError, MediaSite, NewProject, WorkflowMode},
    infrastructure::{sha256_file, ArtifactRegistry, ProjectLayout, ProjectService},
    media::{
        DownloadAdapterContract, DownloaderError, FfmpegAdapter, FfprobeAdapter, MediaImportLimits,
        MediaImportService, MediaToolError, MediaToolService, NetworkPolicy, RemoteDownloader,
        ResolvedEndpoint, UrlPolicyError,
    },
    persistence::{ArtifactRepository, Database, ProjectRepository},
    processes::{ApprovedTool, ProcessLimits, SupervisedProcess, ToolError, ToolInvocation},
};

struct Harness {
    _temporary: TempDir,
    layout: ProjectLayout,
    projects: ProjectService,
    artifacts: ArtifactRegistry,
    importer: MediaImportService,
}

impl Harness {
    fn new(max_source_bytes: u64) -> Self {
        let temporary = tempfile::tempdir().expect("temporary M2 root");
        let database = Database::in_memory().expect("M2 database");
        let layout = ProjectLayout::new(temporary.path().join("projects")).expect("layout");
        let projects =
            ProjectService::new(ProjectRepository::new(database.clone()), layout.clone());
        let artifacts = ArtifactRegistry::new(ArtifactRepository::new(database), layout.clone());
        let importer = MediaImportService::new(
            projects.clone(),
            artifacts.clone(),
            layout.clone(),
            MediaImportLimits::new(max_source_bytes).expect("import limits"),
        );
        Self {
            _temporary: temporary,
            layout,
            projects,
            artifacts,
            importer,
        }
    }

    fn create_project(&self, name: &str) -> Uuid {
        self.projects
            .create(&NewProject::chinese_to_vietnamese(
                name,
                WorkflowMode::Dubbed,
            ))
            .expect("create M2 project")
            .id
    }

    fn external_fixture(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let root = self._temporary.path().join("incoming");
        fs::create_dir_all(&root).expect("incoming root");
        let path = root.join(name);
        fs::write(&path, bytes).expect("write incoming fixture");
        path
    }
}

#[test]
fn local_import_is_immutable_generated_and_shell_inert() {
    let harness = Harness::new(1024);
    let project_id = harness.create_project("Local import");
    let source = harness.external_fixture("video & calc ; $(ignored).mp4", b"generated-video");

    let artifact = harness
        .importer
        .import_local(project_id, &source)
        .expect("import local fixture");

    assert_eq!(artifact.kind, ArtifactKind::SourceVideo);
    assert!(artifact.relative_path.starts_with("source/"));
    assert!(!artifact.relative_path.contains("calc"));
    assert_eq!(artifact.metadata["origin"], "local");
    assert_eq!(
        harness
            .artifacts
            .verify(artifact.id)
            .expect("verify imported source"),
        ArtifactVerification::Verified
    );
    let imported = harness
        .layout
        .project_root(project_id)
        .expect("project root")
        .join(&artifact.relative_path);
    assert!(fs::metadata(imported)
        .expect("import metadata")
        .permissions()
        .readonly());
    assert_eq!(
        fs::read(source).expect("original remains"),
        b"generated-video"
    );
    assert_eq!(
        harness
            .projects
            .get(project_id)
            .expect("project source")
            .source_asset_id,
        Some(artifact.id)
    );
}

#[test]
fn local_import_enforces_size_type_and_single_source_without_partial_files() {
    let harness = Harness::new(8);
    let too_large_project = harness.create_project("Size limit");
    let too_large = harness.external_fixture("large.mp4", b"123456789");
    assert!(matches!(
        harness.importer.import_local(too_large_project, &too_large),
        Err(CoreError::SourceTooLarge)
    ));
    assert_eq!(
        fs::read_dir(
            harness
                .layout
                .project_root(too_large_project)
                .expect("large project")
                .join("source")
        )
        .expect("source directory")
        .count(),
        0
    );

    let unsupported_project = harness.create_project("Unsupported type");
    let unsupported = harness.external_fixture("notes.txt", b"bad");
    assert!(matches!(
        harness
            .importer
            .import_local(unsupported_project, &unsupported),
        Err(CoreError::UnsupportedMedia)
    ));

    let single_project = harness.create_project("Single immutable source");
    let first = harness.external_fixture("first.mp4", b"first");
    let second = harness.external_fixture("second.mp4", b"second");
    harness
        .importer
        .import_local(single_project, &first)
        .expect("first source");
    assert!(matches!(
        harness.importer.import_local(single_project, &second),
        Err(CoreError::SourceAlreadySet)
    ));
}

#[test]
fn remote_promotion_requires_contained_generated_staging_and_replaces_name() {
    let harness = Harness::new(1024);
    let project_id = harness.create_project("Remote promotion");
    let root = harness
        .layout
        .project_root(project_id)
        .expect("remote project");
    fs::write(
        root.join("temp").join("download-123.mp4"),
        b"remote-fixture",
    )
    .expect("staged download");

    let artifact = harness
        .importer
        .promote_remote_download(project_id, "temp/download-123.mp4", MediaSite::Douyin)
        .expect("promote remote file");
    assert_eq!(artifact.metadata["origin"], "remote");
    assert_eq!(artifact.metadata["site"], "douyin");
    assert!(!artifact.relative_path.contains("download-123"));
    assert!(!root.join("temp").join("download-123.mp4").exists());

    let other_project = harness.create_project("Traversal rejection");
    assert!(matches!(
        harness.importer.promote_remote_download(
            other_project,
            "../outside.mp4",
            MediaSite::YouTube
        ),
        Err(CoreError::UnsafePath)
    ));
}

#[test]
fn local_import_does_not_call_or_depend_on_remote_downloader() {
    let harness = Harness::new(1024);
    let project_id = harness.create_project("Offline local import");
    let source = harness.external_fixture("offline.mp4", b"offline");
    let calls = Arc::new(AtomicUsize::new(0));
    let downloader = AlwaysFailDownloader::new(calls.clone());

    harness
        .importer
        .import_local(project_id, &source)
        .expect("local import while downloader is unavailable");
    assert_eq!(downloader.contract().site, MediaSite::YouTube);
    assert_eq!(calls.load(Ordering::Acquire), 0);
}

#[test]
fn all_site_contracts_enforce_their_own_https_hosts() {
    for (site, accepted, rejected) in [
        (
            MediaSite::Douyin,
            "https://www.douyin.com/video/1",
            "https://www.youtube.com/watch?v=1",
        ),
        (
            MediaSite::Bilibili,
            "https://www.bilibili.com/video/BV1",
            "https://www.tiktok.com/video/1",
        ),
        (
            MediaSite::YouTube,
            "https://youtu.be/id",
            "https://v.douyin.com/id",
        ),
        (
            MediaSite::TikTok,
            "https://vm.tiktok.com/id",
            "https://b23.tv/id",
        ),
    ] {
        let contract = DownloadAdapterContract::for_site(site, 1024, Duration::from_secs(5))
            .expect("site contract");
        assert_eq!(
            contract.validate_url(accepted).expect("accepted").site(),
            site
        );
        assert!(matches!(
            contract.validate_url(rejected),
            Err(DownloaderError::WrongSite)
        ));
    }
}

#[test]
fn redirect_and_ssrf_policy_rejects_each_unsafe_hop() {
    let policy = NetworkPolicy::new(3).expect("network policy");
    let initial = policy
        .validate_url("https://youtu.be/id")
        .expect("initial URL");
    let redirect = policy
        .validate_redirect(&initial, 1, "https://www.youtube.com/watch?v=id")
        .expect("same-site redirect");
    assert!(policy
        .validate_addresses(redirect.clone(), ["8.8.8.8".parse::<IpAddr>().expect("IP")])
        .is_ok());
    assert_eq!(
        policy.validate_addresses(
            redirect,
            ["169.254.169.254".parse::<IpAddr>().expect("metadata IP")]
        ),
        Err(UrlPolicyError::NonPublicAddress)
    );
    assert_eq!(
        policy.validate_redirect(&initial, 2, "https://vm.tiktok.com/id"),
        Err(UrlPolicyError::CrossSiteRedirect)
    );
}

#[tokio::test]
async fn supervisor_checks_exit_output_timeout_cancellation_and_redaction() {
    let success = fake_tool("stdout");
    let default_supervisor = supervisor(Duration::from_secs(3), 1024);
    let output = default_supervisor
        .run(
            &success,
            &ToolInvocation::new(["argument & still-one-value"]),
            CancellationToken::new(),
        )
        .await
        .expect("successful supervised tool");
    assert!(String::from_utf8_lossy(&output.stdout).contains("tool-ok"));

    let stderr = default_supervisor
        .run(
            &fake_tool("stderr"),
            &ToolInvocation::new(std::iter::empty::<OsString>()),
            CancellationToken::new(),
        )
        .await
        .expect("redacted stderr");
    assert_eq!(stderr.safe_stderr, "[REDACTED]");
    assert!(matches!(
        default_supervisor
            .run(
                &fake_tool("fail"),
                &ToolInvocation::new(std::iter::empty::<OsString>()),
                CancellationToken::new()
            )
            .await,
        Err(ToolError::Unsuccessful)
    ));
    assert!(matches!(
        supervisor(Duration::from_secs(3), 32)
            .run(
                &fake_tool("oversize"),
                &ToolInvocation::new(std::iter::empty::<OsString>()),
                CancellationToken::new()
            )
            .await,
        Err(ToolError::OutputLimit)
    ));
    assert!(matches!(
        supervisor(Duration::from_millis(50), 1024)
            .run(
                &fake_tool("sleep"),
                &ToolInvocation::new(std::iter::empty::<OsString>()),
                CancellationToken::new()
            )
            .await,
        Err(ToolError::Timeout)
    ));
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    assert!(matches!(
        default_supervisor
            .run(
                &fake_tool("sleep"),
                &ToolInvocation::new(std::iter::empty::<OsString>()),
                cancellation
            )
            .await,
        Err(ToolError::Cancelled)
    ));
}

#[test]
fn approved_tool_detects_post_approval_executable_mutation() {
    let temporary = tempfile::tempdir().expect("integrity root");
    let executable = temporary.path().join("fixture-tool.exe");
    fs::write(&executable, b"version-one").expect("write approved executable");
    let approved = ApprovedTool::new(
        &executable,
        sha256_file(&executable).expect("initial executable hash").0,
    )
    .expect("approve fixture executable");
    fs::write(&executable, b"version-two").expect("mutate approved executable");

    let runtime = tokio::runtime::Runtime::new().expect("test runtime");
    let result = runtime.block_on(supervisor(Duration::from_secs(1), 1024).run(
        &approved,
        &ToolInvocation::new(std::iter::empty::<OsString>()),
        CancellationToken::new(),
    ));
    assert!(matches!(result, Err(ToolError::ExecutableIntegrity)));
}

#[cfg(windows)]
#[tokio::test]
async fn timeout_terminates_the_windows_descendant_process_tree() {
    let temporary = tempfile::tempdir().expect("process-tree root");
    let sentinel = temporary.path().join("descendant.txt");
    let result = supervisor(Duration::from_millis(100), 1024)
        .run(
            &fake_tool("spawn-child"),
            &ToolInvocation::new([sentinel.as_os_str()]),
            CancellationToken::new(),
        )
        .await;
    assert!(matches!(result, Err(ToolError::Timeout)));
    tokio::time::sleep(Duration::from_millis(1_300)).await;
    assert!(
        !sentinel.exists(),
        "descendant escaped its Windows Job Object"
    );
}

#[tokio::test]
async fn probe_proxy_and_audio_use_checked_tools_and_registered_outputs() {
    let harness = Harness::new(1024);
    let project_id = harness.create_project("Media tools");
    let source = harness.external_fixture("source & inert.mp4", b"generated-source");
    let source = harness
        .importer
        .import_local(project_id, &source)
        .expect("import tool source");
    let service = MediaToolService::new(
        harness.layout.clone(),
        harness.artifacts.clone(),
        FfprobeAdapter::new(fake_tool("probe"), supervisor(Duration::from_secs(3), 4096)),
        FfmpegAdapter::new(
            fake_tool("ffmpeg"),
            supervisor(Duration::from_secs(3), 4096),
        ),
    );

    let (metadata, probe_artifact) = service
        .probe_source(project_id, source.id, CancellationToken::new())
        .await
        .expect("probe source");
    assert_eq!(metadata.duration_ms, 2_500);
    assert_eq!(probe_artifact.kind, ArtifactKind::Metadata);
    let probe_path = harness
        .layout
        .project_root(project_id)
        .expect("probe project")
        .join(&probe_artifact.relative_path);
    let probe_value: serde_json::Value =
        serde_json::from_slice(&fs::read(probe_path).expect("read probe metadata artifact"))
            .expect("parse probe metadata artifact");
    let schema: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../schemas/media-metadata.schema.json"
    ))
    .expect("parse media metadata schema");
    jsonschema::validator_for(&schema)
        .expect("media metadata validator")
        .validate(&probe_value)
        .expect("probe artifact matches schema");
    let (_, repeated_probe) = service
        .probe_source(project_id, source.id, CancellationToken::new())
        .await
        .expect("repeat probe without overwriting history");
    assert_ne!(repeated_probe.id, probe_artifact.id);
    assert_ne!(repeated_probe.relative_path, probe_artifact.relative_path);
    let proxy = service
        .create_proxy(project_id, source.id, CancellationToken::new())
        .await
        .expect("create proxy");
    let audio = service
        .extract_normalized_audio(project_id, source.id, CancellationToken::new())
        .await
        .expect("extract normalized audio");
    assert_eq!(proxy.kind, ArtifactKind::ProxyVideo);
    assert_eq!(audio.kind, ArtifactKind::OriginalAudio);
    for artifact in [probe_artifact, proxy, audio] {
        assert_eq!(
            harness
                .artifacts
                .verify(artifact.id)
                .expect("verify media output"),
            ArtifactVerification::Verified
        );
    }
}

#[tokio::test]
async fn ffmpeg_failure_or_missing_output_never_becomes_success() {
    let temporary = tempfile::tempdir().expect("tool failure root");
    let input = temporary.path().join("input.mp4");
    fs::write(&input, b"input").expect("input fixture");
    let failing = FfmpegAdapter::new(
        fake_tool("ffmpeg-fail"),
        supervisor(Duration::from_secs(3), 1024),
    );
    assert!(matches!(
        failing
            .create_proxy(
                &input,
                &temporary.path().join("failed.mp4"),
                CancellationToken::new()
            )
            .await,
        Err(MediaToolError::Tool(ToolError::Unsuccessful))
    ));
    let missing = FfmpegAdapter::new(
        fake_tool("ffmpeg-no-output"),
        supervisor(Duration::from_secs(3), 1024),
    );
    assert!(matches!(
        missing
            .extract_normalized_audio(
                &input,
                &temporary.path().join("missing.wav"),
                CancellationToken::new()
            )
            .await,
        Err(MediaToolError::MissingOutput)
    ));
}

fn supervisor(timeout: Duration, stdout_limit: usize) -> SupervisedProcess {
    SupervisedProcess::new(ProcessLimits {
        timeout,
        max_stdout_bytes: stdout_limit,
        max_stderr_bytes: 4096,
    })
    .expect("process limits")
}

fn fake_tool(mode: &str) -> ApprovedTool {
    let python = python_executable();
    ApprovedTool::with_fixed_args(
        &python,
        sha256_file(&python).expect("hash Python executable").0,
        [
            fixture_root()
                .join("tools/fake_media_tool.py")
                .into_os_string(),
            mode.into(),
        ],
    )
    .expect("approved fake tool")
}

fn fixture_root() -> PathBuf {
    workspace_root().join("tests/fixtures")
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("workspace root")
        .to_path_buf()
}

fn python_executable() -> PathBuf {
    let executable = if cfg!(windows) {
        "python.exe"
    } else {
        "python"
    };
    let workspace_python = if cfg!(windows) {
        workspace_root().join(".venv/Scripts/python.exe")
    } else {
        workspace_root().join(".venv/bin/python")
    };
    std::iter::once(workspace_python)
        .chain(
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
                .map(|path| path.join(executable)),
        )
        .find(|candidate| candidate.is_file())
        .expect("Python executable for test fixture")
        .canonicalize()
        .expect("canonical Python executable")
}

struct AlwaysFailDownloader {
    contract: DownloadAdapterContract,
    calls: Arc<AtomicUsize>,
}

impl AlwaysFailDownloader {
    fn new(calls: Arc<AtomicUsize>) -> Self {
        Self {
            contract: DownloadAdapterContract::for_site(
                MediaSite::YouTube,
                1024,
                Duration::from_secs(1),
            )
            .expect("failing downloader contract"),
            calls,
        }
    }
}

impl RemoteDownloader for AlwaysFailDownloader {
    fn contract(&self) -> &DownloadAdapterContract {
        &self.contract
    }

    fn download<'a>(
        &'a self,
        _initial: &'a ResolvedEndpoint,
        _network_policy: &'a NetworkPolicy,
        _project_temp: &'a Path,
        _cancellation: CancellationToken,
    ) -> Pin<Box<dyn Future<Output = Result<PathBuf, DownloaderError>> + Send + 'a>> {
        self.calls.fetch_add(1, Ordering::AcqRel);
        Box::pin(async { Err(DownloaderError::Failed) })
    }
}
