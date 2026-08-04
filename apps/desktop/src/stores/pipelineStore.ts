import type { PipelineStage, StageStatus, StageSummary } from "../types/pipeline";

export type PipelineAction =
  | { readonly type: "stageUpdated"; readonly stage: PipelineStage; readonly status: StageStatus; readonly progress: number }
  | { readonly type: "reset" };

export function pipelineReducer(
  state: readonly StageSummary[],
  action: PipelineAction,
): readonly StageSummary[] {
  if (action.type === "reset") {
    return [];
  }

  const progress = Math.min(100, Math.max(0, action.progress));
  const next: StageSummary = { stage: action.stage, status: action.status, progress };
  const existingIndex = state.findIndex((item) => item.stage === action.stage);

  if (existingIndex === -1) {
    return [...state, next];
  }

  return state.map((item, index) => (index === existingIndex ? next : item));
}

