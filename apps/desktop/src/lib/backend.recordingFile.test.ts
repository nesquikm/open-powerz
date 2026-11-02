/** The recording-file commands over Tauri IPC (FR AXHK2S). */

import { beforeEach, describe, expect, it, vi } from "vitest";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => {
  class Channel {
    onmessage: (msg: unknown) => void = () => {};
  }
  return { invoke, Channel, isTauri: () => true };
});

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
