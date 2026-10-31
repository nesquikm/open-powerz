/**
 * AC-4PJRVM.2: Record starts at once on a default file; the save dialog comes
 * only at Stop, after the file is complete. A cancelled dialog or a failed
 * move keeps the recording at its default path.
 */
import { describe, expect, it, vi } from "vitest";
import type { RecordingSummary, UiError } from "./backend";
import { Recording, type RecordingDeps } from "./recording.svelte";

const CLOCK = new Date(2026, 9, 21, 9, 5, 7);
const DEFAULT_PATH = "/data/recordings/powerz-20261021-090507.csv";
const CHOSEN_PATH = "/home/me/run.csv";
const SUMMARY: RecordingSummary = { rows: 12, gaps: [], disconnected: false };

function setup(over: Partial<RecordingDeps> = {}) {
  const calls: string[] = [];
  const deps = {
    canRecord: true,
    startRecording: vi.fn(async (name: string) => {
      calls.push(`start ${name}`);
      return DEFAULT_PATH;
    }),
    stopRecording: vi.fn(async () => {
      calls.push("stop");
      return SUMMARY;
    }),
    saveRecordingAs: vi.fn(async (path: string) => {
      calls.push(`save ${path}`);
      return CHOSEN_PATH as string | null;
    }),
    now: () => CLOCK,
    ...over,
  } satisfies RecordingDeps;
  return { deps, calls, recording: new Recording() };
}

describe("Record starts at once and asks where to save at Stop", () => {
  it("does not ask where to save when it starts", async () => {
    const { deps, recording } = setup();
    await recording.toggle(deps);
    expect(recording.phase).toBe("recording");
    expect(recording.path).toBe(DEFAULT_PATH);
    expect(deps.saveRecordingAs).not.toHaveBeenCalled();
  });

  it("asks after the stop, proposing the default file, and names the chosen file", async () => {
    const { deps, calls, recording } = setup();
    await recording.toggle(deps);
    await recording.toggle(deps);
    expect(calls).toEqual(["start powerz-20261021-090507.csv", "stop", `save ${DEFAULT_PATH}`]);
    expect(recording.phase).toBe("idle");
    expect(recording.savedPath).toBe(CHOSEN_PATH);
    expect(recording.notice).toContain(CHOSEN_PATH);
  });

  it("names the chosen file when the meter is unplugged during the dialog", async () => {
    let answer: (path: string | null) => void = () => {};
    const { deps, recording } = setup({
      saveRecordingAs: () => new Promise((r) => (answer = r)),
    });
    await recording.toggle(deps);
    const stopping = recording.toggle(deps);
    await vi.waitFor(() => expect(deps.stopRecording).toHaveBeenCalled());
    await Promise.resolve();
    recording.handleEvent({ state: "stopped", ...SUMMARY });
    expect(recording.savedPath).toBe(DEFAULT_PATH);
    answer(CHOSEN_PATH);
    await stopping;
    expect(recording.savedPath).toBe(CHOSEN_PATH);
    expect(recording.notice).toContain(CHOSEN_PATH);
  });

  it("asks at Stop for a recording picked up after a reload", async () => {
    const { deps, recording } = setup();
    recording.adopt({ rows: 5, elapsed_ms: 900, path: DEFAULT_PATH });
    await recording.toggle(deps);
    expect(deps.saveRecordingAs).toHaveBeenCalledWith(DEFAULT_PATH);
    expect(recording.savedPath).toBe(CHOSEN_PATH);
  });

  it("stays stopping while the dialog is open", async () => {
    let answer: (path: string | null) => void = () => {};
    const { deps, recording } = setup({
      saveRecordingAs: () => new Promise((r) => (answer = r)),
    });
    await recording.toggle(deps);
    const stopping = recording.toggle(deps);
    await vi.waitFor(() => expect(deps.stopRecording).toHaveBeenCalled());
    await Promise.resolve();
    expect(recording.phase).toBe("stopping");
    answer(CHOSEN_PATH);
    await stopping;
    expect(recording.phase).toBe("idle");
  });

  it("keeps the default file when the dialog is cancelled", async () => {
    const { deps, recording } = setup({ saveRecordingAs: vi.fn(async () => null) });
    await recording.toggle(deps);
    await recording.toggle(deps);
    expect(deps.saveRecordingAs).toHaveBeenCalledOnce();
    expect(recording.savedPath).toBe(DEFAULT_PATH);
    expect(recording.notice).toContain(DEFAULT_PATH);
    expect(recording.error).toBeNull();
  });

  it("names the default file and the reason when the move fails", async () => {
    const denied: UiError = { kind: "internal", message: "Cannot write there.", hint: null };
    const { deps, recording } = setup({
      saveRecordingAs: vi.fn(async () => Promise.reject(denied)),
    });
    await recording.toggle(deps);
    await recording.toggle(deps);
    expect(recording.error?.message).toBe("Cannot write there.");
    expect(recording.savedPath).toBe(DEFAULT_PATH);
  });

  it("does not ask when the recording ended without a Stop (an unplug)", async () => {
    const { deps, recording } = setup();
    await recording.toggle(deps);
    recording.handleEvent({ state: "stopped", ...SUMMARY, disconnected: true });
    await Promise.resolve();
    expect(deps.saveRecordingAs).not.toHaveBeenCalled();
    expect(recording.savedPath).toBe(DEFAULT_PATH);
  });
});
