use std::{
    collections::{BTreeMap, HashSet},
    env, fs,
    path::{Path, PathBuf},
    time::Instant,
};

use serde_json::{json, Map};
use sha2::{Digest, Sha256};
use uuid::Uuid;
use vietdub_desktop_lib::{
    domain::{
        CacheDescriptor, CoreError, NewProject, NewStageRun, StageName, StageScope, WorkflowMode,
    },
    hardening::{
        verify_release_artifact, PerformanceBudget, PrivacyLog, PrivacySettings, ReleaseManifest,
        RuntimeSessionGuard,
    },
    infrastructure::{sha256_file, ModelManager, ProjectLayout, ProjectService},
    jobs::PersistentQueue,
    persistence::{Database, ProjectRepository},
    security::{CredentialReference, CredentialStore, SecretString},
};

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .unwrap()
        .to_path_buf()
}

#[test]
fn migration_eight_adds_release_hardening_state() {
    let database = Database::in_memory().unwrap();
    assert_eq!(database.schema_version().unwrap(), 8);
    assert_eq!(PrivacySettings::load(&database).unwrap().max_log_files, 5);
}

#[test]
fn privacy_log_redacts_rotates_and_honors_disable() {
    let temporary = tempfile::tempdir().unwrap();
    let settings = PrivacySettings {
        metadata_logging_enabled: true,
        max_log_files: 2,
        max_log_file_bytes: 64 * 1024,
    };
    let logger = PrivacyLog::new(temporary.path().join("logs"), settings).unwrap();
    for index in 0..260 {
        logger
            .write_event(
                "SECURITY_TEST_EVENT",
                &BTreeMap::from([
                    ("index".into(), index.to_string()),
                    (
                        "credential".into(),
                        "Authorization: Bearer sk-fixture-secret".into(),
                    ),
                    ("input".into(), r"C:\Users\Private\video.mp4".into()),
                    ("padding".into(), "x".repeat(256)),
                ]),
            )
            .unwrap();
    }
    let files = fs::read_dir(logger.root())
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(files.len() <= 2);
    let combined = files
        .iter()
        .map(|entry| fs::read_to_string(entry.path()).unwrap())
        .collect::<String>();
    assert!(!combined.contains("sk-fixture-secret"));
    assert!(!combined.contains(r"C:\Users\Private"));
    assert!(combined.contains("[REDACTED]"));
    assert!(combined.contains("[PATH]"));

    let disabled = PrivacyLog::new(
        temporary.path().join("disabled"),
        PrivacySettings {
            metadata_logging_enabled: false,
            ..settings
        },
    )
    .unwrap();
    disabled
        .write_event("DISABLED_EVENT", &BTreeMap::new())
        .unwrap();
    assert_eq!(fs::read_dir(disabled.root()).unwrap().count(), 0);
}

#[test]
fn crash_recovery_marks_sessions_and_removes_only_partial_files() {
    let temporary = tempfile::tempdir().unwrap();
    let database = Database::in_memory().unwrap();
    let layout = ProjectLayout::new(temporary.path().join("projects")).unwrap();
    let project = ProjectService::new(ProjectRepository::new(database.clone()), layout.clone())
        .create(&NewProject::chinese_to_vietnamese(
            "Recovery",
            WorkflowMode::Dubbed,
        ))
        .unwrap();
    let crashed = RuntimeSessionGuard::begin(database.clone(), &layout).unwrap();
    std::mem::forget(crashed);
    let root = layout.project_root(project.id).unwrap();
    fs::write(root.join("renders/.crashed.partial.mp4"), b"partial").unwrap();
    fs::write(root.join("renders/keep.mp4"), b"complete").unwrap();
    let guard = RuntimeSessionGuard::begin(database.clone(), &layout).unwrap();
    assert_eq!(guard.summary().interrupted_sessions, 1);
    assert_eq!(guard.summary().partial_files_removed, 1);
    assert!(!root.join("renders/.crashed.partial.mp4").exists());
    assert!(root.join("renders/keep.mp4").exists());
    guard.finish().unwrap();
    assert!(guard.is_clean().unwrap());
}

#[test]
fn model_catalog_refuses_unapproved_and_corrupt_installations() {
    let temporary = tempfile::tempdir().unwrap();
    let database = Database::in_memory().unwrap();
    let manager = ModelManager::load(
        database,
        &repository_root().join("resources/manifests/models"),
    )
    .unwrap();
    assert_eq!(manager.list().len(), 4);
    let funasr = temporary.path().join("funasr");
    fs::create_dir(&funasr).unwrap();
    assert!(matches!(
        manager.verify_installation("funasr:paraformer-zh", &funasr),
        Err(CoreError::InvalidInput(_))
    ));

    let model = manager.get("faster-whisper:large-v3").unwrap();
    let installed = temporary.path().join("whisper");
    fs::create_dir(&installed).unwrap();
    let weights = b"deterministic release model fixture";
    fs::write(installed.join("model.bin"), weights).unwrap();
    let manifest = json!({
        "schema_version": 1,
        "model_id": model.model_id,
        "provider": model.provider,
        "version": model.version,
        "license": model.license,
        "source_url": model.source_url,
        "files": [{
            "relative_path": "model.bin",
            "sha256": format!("{:x}", Sha256::digest(weights)),
            "size_bytes": weights.len(),
        }]
    });
    fs::write(
        installed.join("vietdub-model.json"),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
    let report = manager
        .verify_installation("faster-whisper:large-v3", &installed)
        .unwrap();
    assert_eq!(report.file_count, 1);
    fs::write(installed.join("model.bin"), b"corrupt").unwrap();
    assert!(matches!(
        manager.verify_installation("faster-whisper:large-v3", &installed),
        Err(CoreError::ArtifactIntegrity)
    ));
    let traversal = json!({
        "schema_version": 1,
        "model_id": model.model_id,
        "provider": model.provider,
        "version": model.version,
        "license": model.license,
        "source_url": model.source_url,
        "files": [{
            "relative_path": "../outside.bin",
            "sha256": format!("{:064x}", 0),
            "size_bytes": 1,
        }]
    });
    fs::write(
        installed.join("vietdub-model.json"),
        serde_json::to_vec(&traversal).unwrap(),
    )
    .unwrap();
    assert!(manager
        .verify_installation("faster-whisper:large-v3", &installed)
        .is_err());
}

#[cfg(windows)]
#[test]
fn windows_credential_manager_round_trip_or_fails_closed() {
    use vietdub_desktop_lib::security::WindowsCredentialStore;

    let store = WindowsCredentialStore;
    let reference = CredentialReference::new(
        "vietdub.security-test",
        format!("fixture-{}", Uuid::new_v4()),
    )
    .unwrap();
    let secret = SecretString::new("credential-manager-fixture-secret").unwrap();
    if let Err(error) = store.put(&reference, &secret) {
        // Headless Windows agents may not expose a logon-session credential vault.
        // The production adapter must fail closed in that environment rather than
        // falling back to SQLite or a plaintext file.
        assert!(matches!(error, CoreError::CredentialUnavailable));
        assert!(store.get(&reference).is_err());
        return;
    }
    let loaded = store.get(&reference).unwrap();
    assert_eq!(loaded, secret);
    store.delete(&reference).unwrap();
    assert!(store.get(&reference).is_err());
}

#[test]
fn release_manifest_rejects_unsigned_and_checksum_mismatched_artifacts() {
    let temporary = tempfile::tempdir().unwrap();
    let artifact = temporary.path().join("VietDub-Studio_0.1.0_x64-setup.exe");
    fs::write(&artifact, b"unsigned fixture").unwrap();
    let (sha256, size_bytes) = sha256_file(&artifact).unwrap();
    let manifest = ReleaseManifest {
        schema_version: 1,
        product: "VietDub Studio".into(),
        version: "0.1.0".into(),
        channel: "stable".into(),
        artifact_filename: artifact.file_name().unwrap().to_string_lossy().into(),
        sha256,
        size_bytes,
        authenticode_required: true,
        automatic_updates: false,
    };
    assert!(matches!(
        verify_release_artifact(&manifest, &artifact),
        Err(CoreError::ArtifactIntegrity)
    ));
    let mut unsafe_manifest = manifest;
    unsafe_manifest.sha256 = "0".repeat(64);
    assert!(matches!(
        verify_release_artifact(&unsafe_manifest, &artifact),
        Err(CoreError::ArtifactIntegrity)
    ));
    unsafe_manifest.sha256 = sha256_file(&artifact).unwrap().0;
    unsafe_manifest.automatic_updates = true;
    assert!(matches!(
        verify_release_artifact(&unsafe_manifest, &artifact),
        Err(CoreError::InvalidInput(_))
    ));
}

#[test]
fn sbom_is_reconciled_unique_and_contains_no_blocked_license() {
    let sbom: serde_json::Value = serde_json::from_slice(
        &fs::read(repository_root().join("docs/release/sbom.cdx.json")).unwrap(),
    )
    .unwrap();
    let components = sbom["components"].as_array().unwrap();
    assert!(components.len() > 100);
    let mut purls = HashSet::new();
    for component in components {
        let purl = component["purl"].as_str().unwrap();
        assert!(purls.insert(purl));
        let license = component["licenses"][0]["expression"]
            .as_str()
            .unwrap()
            .to_ascii_uppercase();
        for blocked in ["AGPL", "SSPL", "BUSL", "ELASTIC LICENSE"] {
            assert!(!license.contains(blocked), "{purl}: {license}");
        }
    }
    for required in ["pkg:cargo/tauri@", "pkg:npm/react@", "pkg:pypi/pytest@"] {
        assert!(
            purls.iter().any(|value| value.starts_with(required)),
            "{required}"
        );
    }
}

#[test]
fn reference_performance_budgets_are_measured() {
    let budget = PerformanceBudget::default().validate().unwrap();
    let temporary = tempfile::tempdir().unwrap();
    let database_start = Instant::now();
    let database = Database::open(&temporary.path().join("profile.sqlite3")).unwrap();
    assert!(database_start.elapsed() <= budget.startup);
    drop(database);

    let fixture = temporary.path().join("64mib.bin");
    fs::write(&fixture, vec![0x5a; 64 * 1024 * 1024]).unwrap();
    let hash_start = Instant::now();
    let (digest, size) = sha256_file(&fixture).unwrap();
    assert!(hash_start.elapsed() <= budget.artifact_hash_64_mib);
    assert_eq!(size, 64 * 1024 * 1024);
    assert_eq!(digest.len(), 64);

    let database = Database::in_memory().unwrap();
    let layout = ProjectLayout::new(temporary.path().join("queue-projects")).unwrap();
    let project = ProjectService::new(ProjectRepository::new(database.clone()), layout)
        .create(&NewProject::chinese_to_vietnamese(
            "Recovery profile",
            WorkflowMode::Dubbed,
        ))
        .unwrap();
    let queue = PersistentQueue::new(database, 1_000).unwrap();
    for seed in 0_u64..1_000 {
        let cache = CacheDescriptor {
            schema_version: 1,
            input_hash: format!("{seed:064x}"),
            config_hash: format!("{:064x}", seed + 1),
            engine_name: "release-profile".into(),
            engine_version: "1".into(),
            model_version: "none".into(),
            metadata: Map::new(),
        };
        queue
            .enqueue(
                &NewStageRun::new(
                    project.id,
                    StageName::Transcribe,
                    StageScope::Project,
                    cache,
                    "release-profile",
                ),
                0,
            )
            .unwrap();
    }
    for _ in 0..1_000 {
        assert!(queue.claim_next().unwrap().is_some());
    }
    let recovery_start = Instant::now();
    let report = queue.recover_interrupted().unwrap();
    assert!(recovery_start.elapsed() <= budget.queue_recovery);
    assert_eq!(report.requeued, 1_000);
}
