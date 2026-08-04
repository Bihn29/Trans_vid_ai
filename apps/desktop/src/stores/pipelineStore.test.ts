import { pipelineReducer } from "./pipelineStore";

describe("pipelineReducer", () => {
  it("adds, updates, and clamps a stage summary", () => {
    const added = pipelineReducer([], {
      type: "stageUpdated",
      stage: "TRANSCRIBE",
      status: "running",
      progress: 140,
    });
    expect(added).toEqual([{ stage: "TRANSCRIBE", status: "running", progress: 100 }]);

    const updated = pipelineReducer(added, {
      type: "stageUpdated",
      stage: "TRANSCRIBE",
      status: "review_required",
      progress: 82,
    });
    expect(updated).toEqual([{ stage: "TRANSCRIBE", status: "review_required", progress: 82 }]);
  });

  it("resets without mutating the previous state", () => {
    const state = [{ stage: "IMPORT" as const, status: "completed" as const, progress: 100 }];
    expect(pipelineReducer(state, { type: "reset" })).toEqual([]);
    expect(state).toHaveLength(1);
  });

  it("stays within the 100 ms interaction budget", () => {
    let state = [] as ReturnType<typeof pipelineReducer>;
    const started = performance.now();
    for (let index = 0; index < 10_000; index += 1) {
      state = pipelineReducer(state, {
        type: "stageUpdated",
        stage: "TRANSCRIBE",
        status: "running",
        progress: index % 101,
      });
    }
    expect(performance.now() - started).toBeLessThan(100);
    expect(state).toHaveLength(1);
  });
});
