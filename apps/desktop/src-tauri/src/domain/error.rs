use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("database operation failed")]
    Database(#[source] rusqlite::Error),
    #[error("filesystem operation failed")]
    Io(#[source] std::io::Error),
    #[error("shared state is unavailable")]
    LockPoisoned,
    #[error("{0} was not found")]
    NotFound(&'static str),
    #[error("{0} already exists")]
    Conflict(&'static str),
    #[error("invalid {0}")]
    InvalidInput(&'static str),
    #[error("path is outside the project boundary")]
    UnsafePath,
    #[error("artifact integrity verification failed")]
    ArtifactIntegrity,
    #[error("state transition is not allowed")]
    InvalidTransition,
    #[error("a running stage must be stopped before invalidation")]
    RunningStageConflict,
    #[error("the media source exceeds the configured size limit")]
    SourceTooLarge,
    #[error("the media source type is unsupported")]
    UnsupportedMedia,
    #[error("the project already has an immutable source")]
    SourceAlreadySet,
    #[error("approved FFmpeg and ffprobe installations are unavailable")]
    MediaToolsUnavailable,
    #[error("no verified speech recognition model is available")]
    AsrModelUnavailable,
    #[error("speech recognition model could not be loaded")]
    AsrModelLoadFailed,
    #[error("speech recognition failed")]
    AsrTranscriptionFailed,
    #[error("speech recognition worker failed internally")]
    AsrInternalFailure,
    #[error("no verified Chinese to Vietnamese translation model is available")]
    TranslationModelUnavailable,
    #[error("user has not consented to the required model")]
    ModelNotConsented,
    #[error("segment timestamps overlap or are invalid")]
    SegmentOverlap,
    #[error("transcript is in reviewed state and cannot be auto-modified")]
    TranscriptLocked,
    #[error("worker execution failed")]
    WorkerExecution,
    #[error("worker process could not be started")]
    WorkerStartFailed,
    #[error("worker protocol failed")]
    WorkerProtocolFailed,
    #[error("worker response exceeded the message limit")]
    WorkerMessageTooLarge,
    #[error("worker returned an invalid message")]
    WorkerInvalidMessage,
    #[error("worker response did not match the request")]
    WorkerRequestMismatch,
    #[error("worker protocol version did not match")]
    WorkerVersionMismatch,
    #[error("worker emitted a duplicate terminal event")]
    WorkerDuplicateTerminal,
    #[error("worker ended without a terminal event")]
    WorkerMissingTerminal,
    #[error("worker timed out")]
    WorkerTimeout,
    #[error("worker was cancelled")]
    WorkerCancelled,
    #[error("worker process exited unsuccessfully")]
    WorkerProcessExited,
    #[error("worker reported a processing failure")]
    WorkerReportedFailure,
    #[error("translation provider credential is unavailable")]
    CredentialUnavailable,
    #[error("translation provider output is invalid")]
    InvalidTranslationOutput,
}

impl CoreError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Database(_) => "DATABASE_ERROR",
            Self::Io(_) => "FILESYSTEM_ERROR",
            Self::LockPoisoned => "STATE_UNAVAILABLE",
            Self::NotFound(_) => "NOT_FOUND",
            Self::Conflict(_) => "CONFLICT",
            Self::InvalidInput(_) => "INVALID_INPUT",
            Self::UnsafePath => "UNSAFE_PATH",
            Self::ArtifactIntegrity => "ARTIFACT_INTEGRITY_FAILED",
            Self::InvalidTransition => "INVALID_STATE_TRANSITION",
            Self::RunningStageConflict => "RUNNING_STAGE_CONFLICT",
            Self::SourceTooLarge => "SOURCE_TOO_LARGE",
            Self::UnsupportedMedia => "UNSUPPORTED_MEDIA",
            Self::SourceAlreadySet => "SOURCE_ALREADY_SET",
            Self::MediaToolsUnavailable => "MEDIA_TOOLS_UNAVAILABLE",
            Self::AsrModelUnavailable => "ASR_MODEL_UNAVAILABLE",
            Self::AsrModelLoadFailed => "ASR_MODEL_LOAD_FAILED",
            Self::AsrTranscriptionFailed => "ASR_TRANSCRIPTION_FAILED",
            Self::AsrInternalFailure => "ASR_INTERNAL_FAILURE",
            Self::TranslationModelUnavailable => "TRANSLATION_MODEL_UNAVAILABLE",
            Self::ModelNotConsented => "MODEL_NOT_CONSENTED",
            Self::SegmentOverlap => "SEGMENT_OVERLAP",
            Self::TranscriptLocked => "TRANSCRIPT_LOCKED",
            Self::WorkerExecution => "WORKER_EXECUTION_FAILED",
            Self::WorkerStartFailed => "WORKER_START_FAILED",
            Self::WorkerProtocolFailed => "WORKER_PROTOCOL_FAILED",
            Self::WorkerMessageTooLarge => "WORKER_MESSAGE_TOO_LARGE",
            Self::WorkerInvalidMessage => "WORKER_INVALID_MESSAGE",
            Self::WorkerRequestMismatch => "WORKER_REQUEST_MISMATCH",
            Self::WorkerVersionMismatch => "WORKER_VERSION_MISMATCH",
            Self::WorkerDuplicateTerminal => "WORKER_DUPLICATE_TERMINAL",
            Self::WorkerMissingTerminal => "WORKER_MISSING_TERMINAL",
            Self::WorkerTimeout => "WORKER_TIMEOUT",
            Self::WorkerCancelled => "WORKER_CANCELLED",
            Self::WorkerProcessExited => "WORKER_PROCESS_EXITED",
            Self::WorkerReportedFailure => "WORKER_REPORTED_FAILURE",
            Self::CredentialUnavailable => "CREDENTIAL_UNAVAILABLE",
            Self::InvalidTranslationOutput => "INVALID_TRANSLATION_OUTPUT",
        }
    }
}

impl From<rusqlite::Error> for CoreError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Database(error)
    }
}

impl From<std::io::Error> for CoreError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}
