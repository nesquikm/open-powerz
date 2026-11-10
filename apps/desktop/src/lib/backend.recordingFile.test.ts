/** The recording-file commands over Tauri IPC (FR AXHK2S). */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { invoke, fakeWeb, createWebBackend } = vi.hoisted(() => {
  const fakeWeb = {
    features: null as unknown,
    openRecordingFile: vi.fn(async (onProgress: (p: unknown) => void) => {
      onProgress({ done: 0, total: 0 });
      return { name: "run.csv", rows: 3, skipped: 0, left_out: 0, stats: null };
    }),
    cancelRecordingFile: vi.fn(async () => {}),
    closeRecordingFile: vi.fn(async () => {}),
    recordingFileRange: vi.fn(async () => [{ series: "vbus", buckets: [] }]),
    recordingFileRangeStats: vi.fn(async () => ({ samples: 3 })),
    recordingFileRunProgress: vi.fn(async () => ({ samples: 3 })),
  };
  return {
    invoke: vi.fn(),
    fakeWeb,
    createWebBackend: vi.fn(async () => fakeWeb),
  };
});

vi.mock("@tauri-apps/api/core", () => {
  class Channel {
    onmessage: (msg: unknown) => void = () => {};
  }
  return { invoke, Channel, isTauri: () => true };
});

vi.mock("./web/webBackend", () => ({ createWebBackend }));

import {
  cancelRecordingFile,
  closeRecordingFile,
  meterBackend,
  openRecordingFile,
  recordingFileRange,
} from "./backend";

beforeEach(() => {
  invoke.mockReset();
  invoke.mockResolvedValue(null);
});

describe("recording-file commands", () => {
  it("recording_file_open streams progress to onProgress", async () => {
    const seen: unknown[] = [];
    await openRecordingFile((p) => seen.push(p));
    const [command, args] = invoke.mock.calls[0] as [
      string,
      { onProgress: { onmessage: (m: unknown) => void } },
    ];
    expect(command).toBe("recording_file_open");
    args.onProgress.onmessage({ done: 1, total: 2 });
    expect(seen).toEqual([{ done: 1, total: 2 }]);
  });

  it("cancel and close take no meter", async () => {
    await cancelRecordingFile();
    await closeRecordingFile();
    expect(invoke.mock.calls.map((c) => c[0] as string)).toEqual([
      "recording_file_cancel",
      "recording_file_close",
    ]);
  });

  it("recording_file_range carries whole milliseconds, as the Rust side's u64 needs", async () => {
    invoke.mockResolvedValue([]);
    await recordingFileRange(["vbus", "power"], 0.4, 1000.6, 512);
    expect(invoke).toHaveBeenCalledWith(
      "recording_file_range",
      expect.objectContaining({ series: ["vbus", "power"], buckets: 512 }),
    );
    const args = invoke.mock.calls[0]?.[1] as { t0Ms: number; t1Ms: number };
    expect(Number.isInteger(args.t0Ms)).toBe(true);
    expect(Number.isInteger(args.t1Ms)).toBe(true);
  });

  it("export_session_db goes to the given meter, with progress", async () => {
    const seen: unknown[] = [];
    await meterBackend("demo:load-5v-1a").exportSessionDb("run.db", (p) => seen.push(p));
    const [command, args] = invoke.mock.calls[0] as [
      string,
      {
        deviceId: string;
        suggestedName: string;
        onProgress: { onmessage: (m: unknown) => void };
      },
    ];
    expect(command).toBe("export_session_db");
    expect(args.deviceId).toBe("demo:load-5v-1a");
    expect(args.suggestedName).toBe("run.db");
    args.onProgress.onmessage({ done: 1, total: 2 });
    expect(seen).toEqual([{ done: 1, total: 2 }]);
  });
});

/** FR AHTZW3, AC-AHTZW3.1: the web build routes the recording-file commands to the web
 * backend (they used to reject as desktop-only), and never to Tauri. */
describe("recording-file commands in the web build", () => {
  afterEach(() => {
    vi.unstubAllEnvs();
  });

  async function webBackendModule() {
    vi.resetModules();
    vi.stubEnv("VITE_BACKEND", "web");
    vi.clearAllMocks();
    return import("./backend");
  }

  it("Open recording goes to the web backend's picker, with progress", async () => {
    const b = await webBackendModule();
    const seen: unknown[] = [];
    const summary = await b.openRecordingFile((p) => seen.push(p));
    expect(fakeWeb.openRecordingFile).toHaveBeenCalledTimes(1);
    expect(summary?.rows).toBe(3);
    expect(seen).toEqual([{ done: 0, total: 0 }]);
    expect(invoke).not.toHaveBeenCalled();
  });

  it("cancel, close, range, ruler stats and run progress go to the web backend", async () => {
    const b = await webBackendModule();
    await b.cancelRecordingFile();
    await b.closeRecordingFile();
    expect(await b.recordingFileRange(["vbus"], 0, 1000, 10)).toEqual([
      { series: "vbus", buckets: [] },
    ]);
    expect(await b.recordingFileRangeStats(0, 1000)).toEqual({ samples: 3 });
    expect(
      await b.recordingFileRunProgress({ kind: "charge", step_pct: 25, ed_points: 0 }),
    ).toEqual({ samples: 3 });
    expect(fakeWeb.cancelRecordingFile).toHaveBeenCalledTimes(1);
    expect(fakeWeb.closeRecordingFile).toHaveBeenCalledTimes(1);
    expect(fakeWeb.recordingFileRange).toHaveBeenCalledWith(["vbus"], 0, 1000, 10);
    expect(fakeWeb.recordingFileRangeStats).toHaveBeenCalledWith(0, 1000);
    expect(fakeWeb.recordingFileRunProgress).toHaveBeenCalledTimes(1);
    expect(invoke).not.toHaveBeenCalled();
  });
});
