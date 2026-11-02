/**
 * FR AXHK2T (AC-AXHK2T.4, .5): segment marks and E.D. come from the backend's raw
 * samples over Tauri IPC: `run_progress` for a meter's session,
 * `recording_file_run_progress` for an opened recording; both take one `query` and
 * answer a `RunProgress` or `null`.
 */

import { beforeEach, describe, expect, it, vi } from "vitest";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => {
  class Channel {
    onmessage: (msg: unknown) => void = () => {};
  }
  return { invoke, Channel, isTauri: () => true };
});

import { meterBackend, recordingFileRunProgress, runProgress, type RunProgress } from "./backend";

const answer: RunProgress = {
  session_t0_ms: 0,
  run_t0_ms: 0,
  run_t1_ms: 1_000_000,
  samples: 1001,
  charge_ah: 0.2778,
  energy_wh: 1.389,
  marks_total_positive: true,
  marks: [{ t_ms: 500_000, percent: 50 }],
  ed_total_wh: 1.389,
  energy_total_positive: true,
  ed_t_ms: [0, 1_000_000],
  ed_pct: [0, 100],
};

beforeEach(() => {
  invoke.mockReset();
  invoke.mockResolvedValue(answer);
});

describe("run progress commands (AC-AXHK2T.4, .5)", () => {
  it("run_progress sends one whole-number query and answers the backend's progress", async () => {
    await expect(
      runProgress({
        kind: "energy",
        step_pct: 10,
        ed_points: 640.7,
        ed_t0_ms: 100.6,
        ed_t1_ms: 900.2,
      }),
    ).resolves.toEqual(answer);
    expect(invoke).toHaveBeenCalledWith("run_progress", {
      query: {
        kind: "energy",
        step_pct: 10,
        ed_points: 640,
        ed_t0_ms: 100,
        ed_t1_ms: 901,
        ed_scope: "session",
      },
    });
  });

  it("a query with no E.D. window sends nulls, and a bad count sends none", async () => {
    await runProgress({ kind: "charge", step_pct: 25, ed_points: Number.NaN });
    expect(invoke).toHaveBeenCalledWith("run_progress", {
      query: {
        kind: "charge",
        step_pct: 25,
        ed_points: 0,
        ed_t0_ms: null,
        ed_t1_ms: null,
        ed_scope: "session",
      },
    });
  });

  it("E.D. is a share of the session unless the run is asked for (AC-AXHK2T.5)", async () => {
    await runProgress({ kind: "charge", step_pct: 10, ed_points: 2, ed_scope: "run" });
    expect(invoke).toHaveBeenCalledWith("run_progress", {
      query: expect.objectContaining({ ed_scope: "run" }),
    });
  });

  it("run_progress goes to the given meter", async () => {
    await meterBackend("demo:load-5v-1a").runProgress({
      kind: "charge",
      step_pct: 10,
      ed_points: 0,
    });
    expect(invoke).toHaveBeenCalledWith(
      "run_progress",
      expect.objectContaining({ deviceId: "demo:load-5v-1a" }),
    );
  });

  it("recording_file_run_progress takes no meter", async () => {
    await recordingFileRunProgress({ kind: "charge", step_pct: 10, ed_points: 3 });
    const [command, args] = invoke.mock.calls[0] as [string, Record<string, unknown>];
    expect(command).toBe("recording_file_run_progress");
    expect(Object.keys(args)).toEqual(["query"]);
  });

  it("an empty history answers null", async () => {
    invoke.mockResolvedValue(null);
    await expect(runProgress({ kind: "charge", step_pct: 10, ed_points: 0 })).resolves.toBeNull();
  });
});
