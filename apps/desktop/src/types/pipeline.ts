export const pipelineStages = [
  "IMPORT",
  "PROBE",
  "NORMALIZE",
  "EXTRACT_AUDIO",
  "SEPARATE_AUDIO",
  "TRANSCRIBE",
  "SEGMENT",
  "TRANSCRIPT_REVIEW",
  "TRANSLATE",
  "TRANSLATION_REVIEW",
  "VOICE_ASSIGNMENT",
  "VOICE_PREVIEW",
  "SYNTHESIZE",
  "FIT_DURATION",
  "MIX_AUDIO",
  "COMPOSE_VIDEO",
  "QUALITY_CHECK",
  "RENDER",
  "COMPLETE",
] as const;

export type PipelineStage = (typeof pipelineStages)[number];

export type StageStatus =
  | "pending"
  | "queued"
  | "running"
  | "review_required"
  | "completed"
  | "failed"
  | "cancelled"
  | "invalidated";

export interface StageSummary {
  readonly stage: PipelineStage;
  readonly status: StageStatus;
  readonly progress: number;
}

