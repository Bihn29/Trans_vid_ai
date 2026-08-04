use std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString},
    path::{Component, Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use futures_util::StreamExt;
use serde_json::{Map, Value};
use thiserror::Error;
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    process::{Child, Command},
    time::{sleep_until, Instant},
};
use tokio_util::{
    codec::{FramedRead, LinesCodec},
    sync::CancellationToken,
};

use super::protocol::{
    ArtifactOutput, ProgressEvent, WorkerEvent, WorkerRequest, PROTOCOL_VERSION,
};

const REQUEST_SCHEMA: &str = include_str!("../../../../../schemas/worker-request.schema.json");
const RESPONSE_SCHEMA: &str = include_str!("../../../../../schemas/worker-response.schema.json");
const DEFAULT_STDERR_LIMIT_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone)]
pub struct WorkerCommand {
    program: PathBuf,
    prefix_args: Vec<OsString>,
    script: PathBuf,
    working_directory: Option<PathBuf>,
    environment: BTreeMap<OsString, OsString>,
}

impl WorkerCommand {
    pub fn new(program: impl Into<PathBuf>, script: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            prefix_args: Vec::new(),
            script: script.into(),
            working_directory: None,
            environment: BTreeMap::new(),
        }
    }

    pub fn with_prefix_args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        self.prefix_args = args.into_iter().map(Into::into).collect();
        self
    }

    pub fn with_working_directory(mut self, directory: impl Into<PathBuf>) -> Self {
        self.working_directory = Some(directory.into());
        self
    }

    pub fn with_environment(mut self, environment: BTreeMap<OsString, OsString>) -> Self {
        self.environment = environment;
        self
    }

    fn configured(&self) -> Command {
        let mut command = Command::new(&self.program);
        command
            .args(&self.prefix_args)
            .arg("-u")
            .arg(&self.script)
            .env("PYTHONUTF8", "1")
            .env("PYTHONDONTWRITEBYTECODE", "1")
            .envs(&self.environment)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        if let Some(directory) = &self.working_directory {
            command.current_dir(directory);
        }
        command
    }

    pub fn program(&self) -> &Path {
        &self.program
    }

    pub fn script(&self) -> &Path {
        &self.script
    }
}

#[derive(Debug, Clone)]
pub struct WorkerClient {
    command: WorkerCommand,
    timeout: Duration,
    max_message_bytes: usize,
    max_stderr_bytes: usize,
}

#[derive(Debug, PartialEq)]
pub struct WorkerRun {
    pub progress_events: Vec<ProgressEvent>,
    pub artifacts: Vec<ArtifactOutput>,
    pub metrics: Map<String, Value>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Error)]
pub enum WorkerClientError {
    #[error("worker request is invalid")]
    InvalidRequest,
    #[error("worker could not be started")]
    Spawn(#[source] std::io::Error),
    #[error("worker input could not be written")]
    Write(#[source] std::io::Error),
    #[error("worker response exceeded the configured limit")]
    MessageTooLarge,
    #[error("worker returned an invalid protocol message")]
    InvalidMessage,
    #[error("worker response did not match the request")]
    RequestMismatch,
    #[error("worker protocol version is unsupported")]
    VersionMismatch,
    #[error("worker emitted more than one terminal event")]
    DuplicateTerminal,
    #[error("worker ended without a terminal event")]
    MissingTerminal,
    #[error("worker timed out")]
    Timeout,
    #[error("worker was cancelled")]
    Cancelled,
    #[error("worker process exited unsuccessfully")]
    ProcessExited,
    #[error("{error_code}: {safe_message}")]
    WorkerFailed {
        error_code: String,
        safe_message: String,
    },
}

enum TerminalEvent {
    Completed {
        artifacts: Vec<ArtifactOutput>,
        metrics: Map<String, Value>,
        warnings: Vec<String>,
    },
    Failed {
        error_code: String,
        safe_message: String,
    },
}

struct ProtocolRead {
    progress_events: Vec<ProgressEvent>,
    terminal: TerminalEvent,
}

impl WorkerClient {
    pub fn new(command: WorkerCommand, timeout: Duration, max_message_bytes: usize) -> Self {
        Self {
            command,
            timeout,
            max_message_bytes,
            max_stderr_bytes: DEFAULT_STDERR_LIMIT_BYTES,
        }
    }

    pub async fn run(
        &self,
        request: &WorkerRequest,
        cancellation: CancellationToken,
    ) -> Result<WorkerRun, WorkerClientError> {
        self.run_with_progress(request, cancellation, |_| {}).await
    }

    pub async fn run_with_progress<F>(
        &self,
        request: &WorkerRequest,
        cancellation: CancellationToken,
        mut on_progress: F,
    ) -> Result<WorkerRun, WorkerClientError>
    where
        F: FnMut(&ProgressEvent),
    {
        validate_request(request)?;
        let encoded = serde_json::to_vec(request).map_err(|_| WorkerClientError::InvalidRequest)?;
        let mut child = self
            .command
            .configured()
            .spawn()
            .map_err(WorkerClientError::Spawn)?;

        let mut stdin = child.stdin.take().ok_or_else(|| {
            WorkerClientError::Spawn(std::io::Error::other("worker stdin unavailable"))
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            WorkerClientError::Spawn(std::io::Error::other("worker stdout unavailable"))
        })?;
        let stderr = child.stderr.take().ok_or_else(|| {
            WorkerClientError::Spawn(std::io::Error::other("worker stderr unavailable"))
        })?;
        let stderr_task = tokio::spawn(read_bounded(stderr, self.max_stderr_bytes));

        if let Err(error) = async {
            stdin.write_all(&encoded).await?;
            stdin.write_all(b"\n").await?;
            stdin.shutdown().await
        }
        .await
        {
            terminate(&mut child).await;
            let _ = stderr_task.await;
            return Err(WorkerClientError::Write(error));
        }

        let deadline = Instant::now() + self.timeout;
        let mut framed = FramedRead::new(
            stdout,
            LinesCodec::new_with_max_length(self.max_message_bytes),
        );
        let protocol = read_protocol(
            &mut framed,
            request,
            &cancellation,
            deadline,
            &mut on_progress,
        )
        .await;

        let protocol = match protocol {
            Ok(protocol) => protocol,
            Err(error) => {
                terminate(&mut child).await;
                let _ = stderr_task.await;
                return Err(error);
            }
        };

        let status = tokio::select! {
            _ = cancellation.cancelled() => {
                terminate(&mut child).await;
                let _ = stderr_task.await;
                return Err(WorkerClientError::Cancelled);
            }
            _ = sleep_until(deadline) => {
                terminate(&mut child).await;
                let _ = stderr_task.await;
                return Err(WorkerClientError::Timeout);
            }
            status = child.wait() => status,
        }
        .map_err(|_| WorkerClientError::ProcessExited)?;

        let _bounded_stderr = stderr_task.await.unwrap_or_default();
        if !status.success() {
            return Err(WorkerClientError::ProcessExited);
        }

        match protocol.terminal {
            TerminalEvent::Completed {
                artifacts,
                metrics,
                warnings,
            } => Ok(WorkerRun {
                progress_events: protocol.progress_events,
                artifacts,
                metrics,
                warnings,
            }),
            TerminalEvent::Failed {
                error_code,
                safe_message,
            } => Err(WorkerClientError::WorkerFailed {
                error_code,
                safe_message,
            }),
        }
    }
}

async fn read_protocol<R>(
    framed: &mut FramedRead<R, LinesCodec>,
    request: &WorkerRequest,
    cancellation: &CancellationToken,
    deadline: Instant,
    on_progress: &mut impl FnMut(&ProgressEvent),
) -> Result<ProtocolRead, WorkerClientError>
where
    R: AsyncRead + Unpin,
{
    let mut progress_events = Vec::new();
    let mut terminal = None;

    loop {
        let next_line = tokio::select! {
            _ = cancellation.cancelled() => return Err(WorkerClientError::Cancelled),
            _ = sleep_until(deadline) => return Err(WorkerClientError::Timeout),
            line = framed.next() => line,
        };

        let Some(line) = next_line else {
            break;
        };
        let line = line.map_err(|_| WorkerClientError::MessageTooLarge)?;
        if terminal.is_some() {
            return Err(WorkerClientError::DuplicateTerminal);
        }

        let value: Value =
            serde_json::from_str(&line).map_err(|_| WorkerClientError::InvalidMessage)?;
        validate_schema(RESPONSE_SCHEMA, &value).map_err(|_| WorkerClientError::InvalidMessage)?;
        validate_response_paths(&value)?;
        let event: WorkerEvent =
            serde_json::from_value(value).map_err(|_| WorkerClientError::InvalidMessage)?;

        if event.protocol_version() != PROTOCOL_VERSION {
            return Err(WorkerClientError::VersionMismatch);
        }
        if event.request_id() != request.request_id {
            return Err(WorkerClientError::RequestMismatch);
        }

        match event {
            WorkerEvent::Progress {
                progress, message, ..
            } => {
                let progress_event = ProgressEvent { progress, message };
                on_progress(&progress_event);
                progress_events.push(progress_event);
            }
            WorkerEvent::Completed {
                artifacts,
                metrics,
                warnings,
                ..
            } => {
                terminal = Some(TerminalEvent::Completed {
                    artifacts,
                    metrics,
                    warnings,
                });
            }
            WorkerEvent::Failed {
                error_code,
                safe_message,
                ..
            } => {
                terminal = Some(TerminalEvent::Failed {
                    error_code,
                    safe_message,
                });
            }
        }
    }

    terminal
        .map(|terminal| ProtocolRead {
            progress_events,
            terminal,
        })
        .ok_or(WorkerClientError::MissingTerminal)
}

fn validate_request(request: &WorkerRequest) -> Result<(), WorkerClientError> {
    let value = serde_json::to_value(request).map_err(|_| WorkerClientError::InvalidRequest)?;
    validate_schema(REQUEST_SCHEMA, &value).map_err(|_| WorkerClientError::InvalidRequest)?;
    if !is_safe_relative_path(&request.output_directory) {
        return Err(WorkerClientError::InvalidRequest);
    }
    Ok(())
}

fn validate_response_paths(value: &Value) -> Result<(), WorkerClientError> {
    let Some(artifacts) = value.get("artifacts").and_then(Value::as_array) else {
        return Ok(());
    };

    if artifacts.iter().any(|artifact| {
        artifact
            .get("relative_path")
            .and_then(Value::as_str)
            .is_none_or(|path| !is_safe_relative_path(path))
    }) {
        return Err(WorkerClientError::InvalidMessage);
    }
    Ok(())
}

fn validate_schema(schema_source: &str, instance: &Value) -> Result<(), ()> {
    let schema: Value = serde_json::from_str(schema_source).map_err(|_| ())?;
    let validator = jsonschema::validator_for(&schema).map_err(|_| ())?;
    validator.validate(instance).map_err(|_| ())
}

fn is_safe_relative_path(value: &str) -> bool {
    let path = Path::new(value);
    !path.is_absolute()
        && !value.contains('\\')
        && path.components().all(|component| match component {
            Component::Normal(segment) => segment != OsStr::new("..") && segment != OsStr::new("."),
            Component::CurDir
            | Component::ParentDir
            | Component::RootDir
            | Component::Prefix(_) => false,
        })
}

async fn terminate(child: &mut Child) {
    let _ = child.kill().await;
    let _ = child.wait().await;
}

async fn read_bounded<R>(reader: R, limit: usize) -> String
where
    R: AsyncRead + Unpin,
{
    let mut bytes = Vec::with_capacity(limit.min(4096));
    let mut limited = reader.take((limit + 1) as u64);
    let _ = limited.read_to_end(&mut bytes).await;
    if bytes.len() > limit {
        bytes.truncate(limit);
    }
    redact_sensitive_text(&String::from_utf8_lossy(&bytes))
}

fn redact_sensitive_text(input: &str) -> String {
    input
        .lines()
        .map(|line| {
            let lower = line.to_ascii_lowercase();
            if lower.contains("authorization")
                || lower.contains("api_key")
                || lower.contains("api-key")
                || lower.contains("secret")
                || line.contains("sk-")
            {
                "[REDACTED]"
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_uses_discrete_arguments_without_a_shell() {
        let command = WorkerCommand::new("python", "workers/echo/main.py")
            .with_prefix_args(["-X", "utf8"])
            .configured();
        let std_command = command.as_std();
        let args = std_command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();

        assert_eq!(std_command.get_program(), OsStr::new("python"));
        assert_eq!(args, ["-X", "utf8", "-u", "workers/echo/main.py"]);
    }

    #[test]
    fn rejects_paths_that_can_escape_or_change_platform_meaning() {
        assert!(is_safe_relative_path("metadata/echo.json"));
        assert!(!is_safe_relative_path("../outside"));
        assert!(!is_safe_relative_path("metadata/../outside"));
        assert!(!is_safe_relative_path("C:/outside"));
        assert!(!is_safe_relative_path("metadata\\outside"));
        assert!(!is_safe_relative_path("/absolute"));
    }

    #[test]
    fn redacts_sensitive_diagnostics() {
        let input = "ordinary warning\nAuthorization: Bearer value\napi_key=secret\nsk-live";
        let redacted = redact_sensitive_text(input);

        assert!(redacted.contains("ordinary warning"));
        assert!(!redacted.contains("Bearer value"));
        assert!(!redacted.contains("api_key"));
        assert!(!redacted.contains("sk-live"));
    }
}
