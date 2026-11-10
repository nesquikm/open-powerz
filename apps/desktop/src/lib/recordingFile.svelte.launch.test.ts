/** FR M078V5, AC-M078V5.1 and .4: a recording handed over at launch opens through the
 * File tab's store by Open recording's rules: a refusal says so in the same words, and
 * the open file is replaced only once the new one has been read. */

import { beforeEach, describe, expect, it, vi } from "vitest";
import type { FileSummary, OpenProgress, UiError } from "./backend";

const backend = vi.hoisted(() => ({
  openRecordingFile: vi.fn(),
  openLaunchRecordingFile: vi.fn(),
  cancelRecordingFile: vi.fn(),
  closeRecordingFile: vi.fn(),
}));

vi.mock("./backend", async (original) => ({
  ...(await original<typeof import("./backend")>()),
  ...backend,
}));

import { RecordingFile } from "./recordingFile.svelte";

function summary(name: string): FileSummary {
  return {
    name,
    rows: 3,
    skipped: 0,
    left_out: 0,
    stats: {
      elapsed_ms: 200,
      samples: 3,
      charge_ah: 0.1,
      energy_wh: 0.5,
      vbus: { min: 5, max: 6, avg: 5.5 },
      ibus: { min: 1, max: 1, avg: 1 },
      power: { min: 5, max: 6, avg: 5.5 },
    },
  } as FileSummary;
}

const REFUSED: UiError = {
  kind: "usage",
  message: "This file's header is not a recorder header.",
  hint: null,
} as UiError;

beforeEach(() => {
  backend.openRecordingFile.mockReset();
  backend.openLaunchRecordingFile.mockReset();
  backend.cancelRecordingFile.mockReset().mockResolvedValue(undefined);
  backend.closeRecordingFile.mockReset().mockResolvedValue(undefined);
});

describe("RecordingFile.openLaunch", () => {
  it("opens the handed-over file without asking through the dialog (AC-M078V5.1)", async () => {
    backend.openLaunchRecordingFile.mockResolvedValueOnce(summary("launch.csv"));
    const file = new RecordingFile();
    expect(await file.openLaunch()).toBe(true);
    expect(file.summary?.name).toBe("launch.csv");
    expect(file.error).toBeNull();
    expect(file.opening).toBe(false);
    expect(backend.openRecordingFile).not.toHaveBeenCalled();
  });

  it("reports progress while it reads, then clears it", async () => {
    let report: (p: OpenProgress) => void = () => {};
    let finish: (s: FileSummary | null) => void = () => {};
    backend.openLaunchRecordingFile.mockImplementationOnce(
      (onProgress: (p: OpenProgress) => void) =>
        new Promise((resolve) => {
          report = onProgress;
          finish = resolve;
        }),
    );
    const file = new RecordingFile();
    const done = file.openLaunch();
    expect(file.opening).toBe(true);
    report({ done: 1, total: 2 });
    expect(file.progress).toEqual({ done: 1, total: 2 });
    expect(file.working).toBe(true);
    finish(summary("launch.csv"));
    expect(await done).toBe(true);
    expect(file.progress).toBeNull();
    expect(file.opening).toBe(false);
  });

  it("with no file handed over, nothing changes (AC-M078V5.2)", async () => {
    backend.openRecordingFile.mockResolvedValueOnce(summary("old.csv"));
    backend.openLaunchRecordingFile.mockResolvedValueOnce(null);
    const file = new RecordingFile();
    await file.open();
    expect(await file.openLaunch()).toBe(false);
    expect(file.summary?.name).toBe("old.csv");
    expect(file.error).toBeNull();
  });

  it("a refused file shows Open recording's error and keeps the open one (AC-M078V5.4)", async () => {
    backend.openRecordingFile.mockResolvedValueOnce(summary("old.csv"));
    backend.openLaunchRecordingFile.mockRejectedValueOnce(REFUSED);
    const file = new RecordingFile();
    await file.open();
    expect(await file.openLaunch()).toBe(false);
    expect(file.error).toBe(REFUSED.message);
    expect(file.summary?.name).toBe("old.csv");
    expect(file.opening).toBe(false);
  });

  it("the open file is replaced once the new one has been read (AC-M078V5.4)", async () => {
    backend.openRecordingFile.mockResolvedValueOnce(summary("old.csv"));
    let finish: (s: FileSummary | null) => void = () => {};
    backend.openLaunchRecordingFile.mockImplementationOnce(
      () => new Promise((resolve) => (finish = resolve)),
    );
    const file = new RecordingFile();
    await file.open();
    const done = file.openLaunch();
    expect(file.summary?.name).toBe("old.csv");
    finish(summary("new.db"));
    expect(await done).toBe(true);
    expect(file.summary?.name).toBe("new.db");
  });

  it("is ignored while another open runs", async () => {
    backend.openRecordingFile.mockImplementationOnce(() => new Promise(() => {}));
    const file = new RecordingFile();
    void file.open();
    expect(await file.openLaunch()).toBe(false);
    expect(backend.openLaunchRecordingFile).not.toHaveBeenCalled();
  });
});
