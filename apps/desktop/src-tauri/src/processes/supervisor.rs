use std::{
    ffi::OsString,
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use sha2::{Digest, Sha256};
use thiserror::Error;
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::{Child, Command},
    time::timeout,
};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone)]
pub struct ApprovedTool {
    program: PathBuf,
    sha256: String,
    fixed_args: Vec<OsString>,
}

impl ApprovedTool {
    pub fn new(program: impl AsRef<Path>, sha256: impl Into<String>) -> Result<Self, ToolError> {
        Self::with_fixed_args(program, sha256, std::iter::empty::<OsString>())
    }

    pub fn with_fixed_args<I, S>(
        program: impl AsRef<Path>,
        sha256: impl Into<String>,
        fixed_args: I,
    ) -> Result<Self, ToolError>
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        let source = program.as_ref();
        if !source.is_absolute() {
            return Err(ToolError::UnapprovedExecutable);
        }
        let source_metadata =
            std::fs::symlink_metadata(source).map_err(|_| ToolError::UnapprovedExecutable)?;
        if source_metadata.file_type().is_symlink() || !source_metadata.is_file() {
            return Err(ToolError::UnapprovedExecutable);
        }
        let program = source
            .canonicalize()
            .map_err(|_| ToolError::UnapprovedExecutable)?;
        let sha256 = sha256.into();
        if sha256.len() != 64
            || !sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
            || hash_file(&program).map_err(|_| ToolError::ExecutableIntegrity)? != sha256
        {
            return Err(ToolError::ExecutableIntegrity);
        }
        Ok(Self {
            program,
            sha256,
            fixed_args: fixed_args.into_iter().map(Into::into).collect(),
        })
    }

    pub fn program(&self) -> &Path {
        &self.program
    }

    fn verify(&self) -> Result<(), ToolError> {
        let metadata =
            std::fs::symlink_metadata(&self.program).map_err(|_| ToolError::ExecutableIntegrity)?;
        if metadata.file_type().is_symlink()
            || !metadata.is_file()
            || hash_file(&self.program).map_err(|_| ToolError::ExecutableIntegrity)? != self.sha256
        {
            return Err(ToolError::ExecutableIntegrity);
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct ToolInvocation {
    pub args: Vec<OsString>,
    pub current_directory: Option<PathBuf>,
}

impl ToolInvocation {
    pub fn new<I, S>(args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        Self {
            args: args.into_iter().map(Into::into).collect(),
            current_directory: None,
        }
    }

    pub fn in_directory(mut self, directory: impl Into<PathBuf>) -> Self {
        self.current_directory = Some(directory.into());
        self
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ProcessLimits {
    pub timeout: Duration,
    pub max_stdout_bytes: usize,
    pub max_stderr_bytes: usize,
}

impl ProcessLimits {
    pub fn validate(self) -> Result<Self, ToolError> {
        if self.timeout.is_zero()
            || self.max_stdout_bytes == 0
            || self.max_stderr_bytes == 0
            || self.max_stdout_bytes > 16 * 1024 * 1024
            || self.max_stderr_bytes > 1024 * 1024
        {
            return Err(ToolError::InvalidLimits);
        }
        Ok(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolOutput {
    pub stdout: Vec<u8>,
    pub safe_stderr: String,
}

#[derive(Debug, Error)]
pub enum ToolError {
    #[error("executable is not an approved canonical file")]
    UnapprovedExecutable,
    #[error("approved executable checksum verification failed")]
    ExecutableIntegrity,
    #[error("process limits are invalid")]
    InvalidLimits,
    #[error("tool working directory is unsafe")]
    UnsafeWorkingDirectory,
    #[error("tool could not be started")]
    Spawn(#[source] std::io::Error),
    #[error("process-tree supervision could not be enabled")]
    Supervision(#[source] std::io::Error),
    #[error("tool timed out")]
    Timeout,
    #[error("tool was cancelled")]
    Cancelled,
    #[error("tool output exceeded its configured limit")]
    OutputLimit,
    #[error("tool exited unsuccessfully")]
    Unsuccessful,
    #[error("tool output could not be read")]
    Output(#[source] std::io::Error),
}

#[derive(Debug, Clone)]
pub struct SupervisedProcess {
    limits: ProcessLimits,
}

impl SupervisedProcess {
    pub fn new(limits: ProcessLimits) -> Result<Self, ToolError> {
        Ok(Self {
            limits: limits.validate()?,
        })
    }

    pub async fn run(
        &self,
        tool: &ApprovedTool,
        invocation: &ToolInvocation,
        cancellation: CancellationToken,
    ) -> Result<ToolOutput, ToolError> {
        tool.verify()?;
        let mut command = Command::new(tool.program());
        command
            .args(&tool.fixed_args)
            .args(&invocation.args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        if let Some(directory) = &invocation.current_directory {
            let canonical = directory
                .canonicalize()
                .map_err(|_| ToolError::UnsafeWorkingDirectory)?;
            if !canonical.is_dir() {
                return Err(ToolError::UnsafeWorkingDirectory);
            }
            command.current_dir(canonical);
        }

        let mut child = command.spawn().map_err(ToolError::Spawn)?;
        let process_tree = match ProcessTree::attach(&child) {
            Ok(process_tree) => process_tree,
            Err(error) => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                return Err(ToolError::Supervision(error));
            }
        };
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| ToolError::Spawn(std::io::Error::other("stdout unavailable")))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| ToolError::Spawn(std::io::Error::other("stderr unavailable")))?;
        let stdout_task = tokio::spawn(read_limited(stdout, self.limits.max_stdout_bytes));
        let stderr_task = tokio::spawn(read_limited(stderr, self.limits.max_stderr_bytes));

        let outcome = tokio::select! {
            _ = cancellation.cancelled() => Err(ToolError::Cancelled),
            result = timeout(self.limits.timeout, child.wait()) => match result {
                Ok(Ok(status)) => Ok(status),
                Ok(Err(error)) => Err(ToolError::Output(error)),
                Err(_) => Err(ToolError::Timeout),
            },
        };

        let status = match outcome {
            Ok(status) => status,
            Err(error) => {
                terminate(&mut child, &process_tree).await;
                let _ = stdout_task.await;
                let _ = stderr_task.await;
                return Err(error);
            }
        };
        let stdout = stdout_task
            .await
            .map_err(|_| ToolError::Output(std::io::Error::other("stdout task failed")))??;
        let stderr = stderr_task
            .await
            .map_err(|_| ToolError::Output(std::io::Error::other("stderr task failed")))??;
        if stdout.truncated || stderr.truncated {
            return Err(ToolError::OutputLimit);
        }
        if !status.success() {
            return Err(ToolError::Unsuccessful);
        }

        Ok(ToolOutput {
            stdout: stdout.bytes,
            safe_stderr: redact_sensitive_text(&String::from_utf8_lossy(&stderr.bytes)),
        })
    }
}

fn hash_file(path: &Path) -> std::io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

struct LimitedOutput {
    bytes: Vec<u8>,
    truncated: bool,
}

async fn read_limited<R>(mut reader: R, limit: usize) -> Result<LimitedOutput, ToolError>
where
    R: AsyncRead + Unpin,
{
    let mut bytes = Vec::with_capacity(limit.min(4096));
    let mut limited = (&mut reader).take((limit + 1) as u64);
    limited
        .read_to_end(&mut bytes)
        .await
        .map_err(ToolError::Output)?;
    let truncated = bytes.len() > limit;
    bytes.truncate(limit);
    Ok(LimitedOutput { bytes, truncated })
}

async fn terminate(child: &mut Child, process_tree: &ProcessTree) {
    process_tree.terminate();
    let _ = child.kill().await;
    let _ = child.wait().await;
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

#[cfg(windows)]
struct ProcessTree {
    handle: windows_sys::Win32::Foundation::HANDLE,
}

// SAFETY: a Windows job HANDLE is a process-wide kernel handle. The wrapper owns it,
// closes it exactly once in Drop, and only performs thread-safe kernel operations on it.
#[cfg(windows)]
unsafe impl Send for ProcessTree {}

// SAFETY: `terminate` does not mutate wrapper memory and Windows permits the HANDLE
// to be referenced from another thread while the owning wrapper remains alive.
#[cfg(windows)]
unsafe impl Sync for ProcessTree {}

#[cfg(windows)]
impl ProcessTree {
    fn attach(child: &Child) -> std::io::Result<Self> {
        use std::{mem::size_of, ptr::null};
        use windows_sys::Win32::{
            Foundation::CloseHandle,
            System::JobObjects::{
                AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
                SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
                JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            },
        };

        // SAFETY: both pointers are null by design for an unnamed job with default security.
        let handle = unsafe { CreateJobObjectW(null(), null()) };
        if handle.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        let mut information = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        information.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // SAFETY: `information` has the exact class layout and remains alive for the call.
        let configured = unsafe {
            SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                (&raw const information).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        let Some(raw_process) = child.raw_handle() else {
            // SAFETY: `handle` was returned by CreateJobObjectW and is still owned here.
            unsafe { CloseHandle(handle) };
            return Err(std::io::Error::other("process handle unavailable"));
        };
        // SAFETY: both handles are live kernel handles owned by this function/child.
        let assigned =
            configured != 0 && unsafe { AssignProcessToJobObject(handle, raw_process.cast()) } != 0;
        if !assigned {
            let error = std::io::Error::last_os_error();
            // SAFETY: assignment failed, so this function still exclusively owns the job handle.
            unsafe { CloseHandle(handle) };
            return Err(error);
        }
        Ok(Self { handle })
    }

    fn terminate(&self) {
        use windows_sys::Win32::System::JobObjects::TerminateJobObject;
        // SAFETY: the handle remains live for the lifetime of ProcessTree.
        unsafe {
            TerminateJobObject(self.handle, 1);
        }
    }
}

#[cfg(windows)]
impl Drop for ProcessTree {
    fn drop(&mut self) {
        // SAFETY: ProcessTree owns this handle exactly once.
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.handle);
        }
    }
}

#[cfg(not(windows))]
struct ProcessTree;

#[cfg(not(windows))]
impl ProcessTree {
    fn attach(_child: &Child) -> std::io::Result<Self> {
        Ok(Self)
    }

    fn terminate(&self) {}
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;

    use super::*;

    #[test]
    fn approved_tool_rejects_relative_and_missing_paths() {
        assert!(matches!(
            ApprovedTool::new("ffmpeg", "0".repeat(64)),
            Err(ToolError::UnapprovedExecutable)
        ));
        assert!(matches!(
            ApprovedTool::new(Path::new("Z:/missing/ffmpeg.exe"), "0".repeat(64)),
            Err(ToolError::UnapprovedExecutable)
        ));
    }

    #[test]
    fn invocation_keeps_shell_metacharacters_in_one_argument() {
        let invocation = ToolInvocation::new([OsStr::new("video & calc.exe ; $(bad).mp4")]);
        assert_eq!(invocation.args.len(), 1);
        assert_eq!(invocation.args[0], "video & calc.exe ; $(bad).mp4");
    }
}
