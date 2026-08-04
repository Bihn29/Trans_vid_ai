mod artifact;
mod audio;
mod composer;
mod error;
mod job;
mod media;
mod model;
mod pipeline;
mod project;
mod transcript;
mod translation;
mod tts;

pub use artifact::{Artifact, ArtifactKind, ArtifactVerification, NewArtifact};
pub use audio::{AudioMixSettings, AudioQualityReport, SeparationEngineDescriptor};
pub use composer::{
    AspectPreset, BlurRegion, ComposerConfig, CoverRegion, CropRect, FlipMode, ImageOverlay,
    ImageOverlayKind, PreviewPreset, RenderQualityReport, SubtitleMode, TextOverlay, TimedRegion,
};
pub use error::CoreError;
pub use job::{Job, JobStatus, NewJob};
pub use media::{MediaMetadata, MediaSite};
pub use model::{ApprovedModelManifest, ModelConsent, ModelInstallationReport, ModelManifest};
pub use pipeline::{CacheDescriptor, NewStageRun, StageName, StageRun, StageScope, StageStatus};
pub use project::{NewProject, Project, ProjectStatus, ProjectUpdate, WorkflowMode};
pub use transcript::{
    check_transcript_quality, compute_text_hash, NewSegment, ReviewStatus, Segment, SegmentUpdate,
    SegmentWarning,
};
pub use translation::{
    GlossaryEntry, LockedProperName, NewTranslationBlock, TranslationBlock, TranslationBlockStatus,
    TranslationItem, TranslationProviderDisclosure, TranslationResult, TranslationSegmentInput,
    MAX_TRANSLATION_BLOCK_SEGMENTS,
};
pub use tts::{
    duration_fit, DurationFit, NewTtsSegmentRun, TtsRunStatus, TtsSegmentRun, VoiceAssignment,
    VoiceDescriptor, VoiceScope,
};
