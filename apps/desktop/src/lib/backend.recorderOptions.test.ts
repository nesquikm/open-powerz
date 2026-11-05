/** The recorder's commands over Tauri IPC (FR AXHK2R). */

import { beforeEach, describe, expect, it, vi } from "vitest";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => {
  class Channel {
    onmessage: (msg: unknown) => void = () => {};
  }
  return { invoke, Channel, isTauri: () => true };
});

import {
  chooseRecordingFolder,
  meterBackend,
  pauseRecording,
  resumeRecording,
  startRecording,
} from "./backend";

const OPTIONS = { folder: "/data/runs", below_a: 0.05, for_ms: 30_000, max_rows: 5_000 };

beforeEach(() => {
  invoke.mockReset();
  invoke.mockResolvedValue(null);
});

describe("recorder commands", () => {
  it("start_recording carries the options", async () => {
    await startRecording("bench.csv", OPTIONS);
    expect(invoke).toHaveBeenCalledWith("start_recording", {
      suggestedName: "bench.csv",
      options: OPTIONS,
    });
  });

  it("pause and resume have their own commands", async () => {
    await pauseRecording();
    await resumeRecording();
    expect(invoke).toHaveBeenNthCalledWith(1, "recording_pause");
    expect(invoke).toHaveBeenNthCalledWith(2, "recording_resume");
  });

  it("the folder comes from its own command", async () => {
    invoke.mockResolvedValue("/data/runs");
    await expect(chooseRecordingFolder()).resolves.toBe("/data/runs");
    expect(invoke).toHaveBeenCalledWith("choose_recording_folder");
  });

  it("a tile's commands name its meter", async () => {
    const a = meterBackend("demo:a");
    await a.startRecording("bench.csv", OPTIONS);
    await a.pauseRecording();
    await a.resumeRecording();
    expect(invoke).toHaveBeenNthCalledWith(1, "start_recording", {
      deviceId: "demo:a",
      suggestedName: "bench.csv",
      options: OPTIONS,
    });
    expect(invoke).toHaveBeenNthCalledWith(2, "recording_pause", { deviceId: "demo:a" });
    expect(invoke).toHaveBeenNthCalledWith(3, "recording_resume", { deviceId: "demo:a" });
  });
});
