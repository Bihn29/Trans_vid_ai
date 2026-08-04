use std::{env, path::PathBuf, process::Command as StdCommand, time::Duration};

use serde_json::json;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
use vietdub_desktop_lib::workers::{WorkerClient, WorkerClientError, WorkerCommand, WorkerRequest};

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("src-tauri is nested under apps/desktop")
        .to_path_buf()
}

fn python_worker(script: &str) -> WorkerCommand {
    let worker_path = repository_root().join(script);
    if let Ok(program) = env::var("VIETDUB_PYTHON") {
        return WorkerCommand::new(program, worker_path);
    }

    let workspace_python = repository_root()
        .join(".venv")
        .join("Scripts")
        .join("python.exe");
    if interpreter_is_python_311(&workspace_python, &[]) {
        return WorkerCommand::new(workspace_python, worker_path);
    }

    for (program, prefix_args) in [
        ("python", Vec::<&str>::new()),
        ("python3", Vec::<&str>::new()),
        ("py", vec!["-3.11"]),
    ] {
        if interpreter_is_python_311(program, &prefix_args) {
            return WorkerCommand::new(program, worker_path).with_prefix_args(prefix_args);
        }
    }

    panic!("Python 3.11 was not found; set VIETDUB_PYTHON to its executable path");
}

fn interpreter_is_python_311(program: impl AsRef<std::ffi::OsStr>, prefix_args: &[&str]) -> bool {
    StdCommand::new(program)
        .args(prefix_args)
        .arg("--version")
        .output()
        .is_ok_and(|output| {
            if !output.status.success() {
                return false;
            }
            let version = format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            version.starts_with("Python 3.11.")
        })
}

fn request(action: &str) -> WorkerRequest {
    WorkerRequest::new(action, Uuid::new_v4(), "metadata/echo")
}

#[tokio::test]
async fn rust_receives_echo_progress_and_result() {
    let client = WorkerClient::new(
        python_worker("workers/echo/main.py"),
        Duration::from_secs(5),
        64 * 1024,
    );
    let mut request = request("echo");
    request.input.insert("value".into(), json!("deterministic"));

    let run = client
        .run(&request, CancellationToken::new())
        .await
        .expect("echo worker succeeds");

    assert_eq!(run.progress_events.len(), 2);
    assert_eq!(run.progress_events[0].progress, 0);
    assert_eq!(run.progress_events[1].progress, 100);
    assert_eq!(run.metrics["worker"], "echo");
    assert_eq!(run.metrics["echo"]["value"], "deterministic");
}

#[tokio::test]
async fn worker_failure_is_typed_and_does_not_crash() {
    let client = WorkerClient::new(
        python_worker("workers/echo/main.py"),
        Duration::from_secs(5),
        64 * 1024,
    );

    let error = client
        .run(&request("fail"), CancellationToken::new())
        .await
        .expect_err("requested worker failure is returned");

    assert!(matches!(
        error,
        WorkerClientError::WorkerFailed { ref error_code, ref safe_message }
            if error_code == "ECHO_REQUESTED_FAILURE"
                && safe_message == "Tác vụ kiểm tra đã trả về lỗi theo yêu cầu."
    ));
}

#[tokio::test]
async fn sleeping_worker_times_out_and_is_reaped() {
    let client = WorkerClient::new(
        python_worker("workers/echo/main.py"),
        Duration::from_millis(100),
        64 * 1024,
    );
    let mut request = request("sleep");
    request.config.insert("delay_ms".into(), json!(5_000));

    let error = client
        .run(&request, CancellationToken::new())
        .await
        .expect_err("sleeping worker times out");

    assert!(matches!(error, WorkerClientError::Timeout));
}

#[tokio::test]
async fn cancellation_stops_only_the_selected_run() {
    let client = WorkerClient::new(
        python_worker("workers/echo/main.py"),
        Duration::from_secs(5),
        64 * 1024,
    );
    let mut request = request("sleep");
    request.config.insert("delay_ms".into(), json!(5_000));
    let cancellation = CancellationToken::new();
    let cancellation_signal = cancellation.clone();

    let run = tokio::spawn(async move { client.run(&request, cancellation).await });
    tokio::time::sleep(Duration::from_millis(100)).await;
    cancellation_signal.cancel();

    let error = run
        .await
        .expect("worker task joins")
        .expect_err("worker run is cancelled");
    assert!(matches!(error, WorkerClientError::Cancelled));
}

#[tokio::test]
async fn oversized_worker_message_is_rejected() {
    let client = WorkerClient::new(
        python_worker("tests/fixtures/workers/oversized_worker.py"),
        Duration::from_secs(5),
        1024,
    );

    let error = client
        .run(&request("echo"), CancellationToken::new())
        .await
        .expect_err("oversized output is rejected");
    assert!(matches!(error, WorkerClientError::MessageTooLarge));
}
