/** The opened-file state (FR AXHK2S, AC .1 to .5 and .7): opening with progress and
 * Cancel, a refusal in the backend's words, Close. One file at a time: a new one replaces
 * the old, and a dialog dismissed or a refusal keeps it. */

import { readFileSync } from "node:fs";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { FileSummary, OpenProgress, UiError } from "./backend";

const backend = vi.hoisted(() => ({
  openRecordingFile: vi.fn(),
  cancelRecordingFile: vi.fn(),
  closeRecordingFile: vi.fn(),
}));

vi.mock("./backend", async (original) => ({
  ...(await original<typeof import("./backend")>()),
  ...backend,
}));

import { RecordingFile } from "./recordingFile.svelte";

function summary(over: Partial<FileSummary> = {}): FileSummary {
  return {
    name: "run.csv",
    rows: 3,
    skipped: 1,
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
    ...over,
  };
}

const REFUSED: UiError = {
  kind: "usage",
  message: "This file's header is not a recorder header.",
  hint: null,
} as UiError;

/** An open held pending: `finish` settles it, `report` sends progress. */
function pendingOpen() {
  let finish: (value: FileSummary | null) => void = () => {};
  let fail: (error: UiError) => void = () => {};
  let report: (p: OpenProgress) => void = () => {};
  backend.openRecordingFile.mockImplementationOnce(
    (onProgress: (p: OpenProgress) => void) =>
      new Promise((resolve, reject) => {
        report = onProgress;
        finish = resolve;
        fail = reject;
      }),
  );
  return {
    finish: (value: FileSummary | null) => finish(value),
    fail: (error: UiError) => fail(error),
    report: (p: OpenProgress) => report(p),
  };
}

beforeEach(() => {
  backend.openRecordingFile.mockReset();
  backend.cancelRecordingFile.mockReset().mockResolvedValue(undefined);
  backend.closeRecordingFile.mockReset().mockResolvedValue(undefined);
});

describe("RecordingFile", () => {
  it("starts with no file", () => {
    const file = new RecordingFile();
    expect(file.summary).toBeNull();
    expect(file.opening).toBe(false);
    expect(file.progress).toBeNull();
    expect(file.error).toBeNull();
  });

  it("shows progress while opening and the summary at the end", async () => {
    const held = pendingOpen();
    const file = new RecordingFile();

    const opening = file.open();
    expect(file.opening).toBe(true);
    held.report({ done: 40, total: 100 });
    expect(file.progress).toEqual({ done: 40, total: 100 });

    const done = summary();
    held.finish(done);
    await opening;

    expect(file.summary).toEqual(done);
    expect(file.opening).toBe(false);
    expect(file.progress).toBeNull();
    expect(file.error).toBeNull();
  });

  it("does nothing more when the open dialog is dismissed", async () => {
    backend.openRecordingFile.mockResolvedValueOnce(null);
    const file = new RecordingFile();
    await file.open();
    expect(file.summary).toBeNull();
    expect(file.opening).toBe(false);
    expect(file.error).toBeNull();
  });

  it("shows a refusal in the backend's words, with no file", async () => {
    backend.openRecordingFile.mockRejectedValueOnce(REFUSED);
    const file = new RecordingFile();
    await file.open();
    expect(file.summary).toBeNull();
    expect(file.opening).toBe(false);
    expect(file.progress).toBeNull();
    expect(file.error).toBe(REFUSED.message);
  });

  it("clears an old refusal when the next open starts", async () => {
    backend.openRecordingFile.mockRejectedValueOnce(REFUSED);
    const file = new RecordingFile();
    await file.open();
    const held = pendingOpen();
    const opening = file.open();
    expect(file.error).toBeNull();
    held.finish(summary());
    await opening;
    expect(file.error).toBeNull();
  });

  it("Cancel tells the backend, and the open that ends in null leaves no file", async () => {
    const held = pendingOpen();
    const file = new RecordingFile();
    const opening = file.open();

    await file.cancel();
    expect(backend.cancelRecordingFile).toHaveBeenCalledTimes(1);

    held.finish(null);
    await opening;
    expect(file.summary).toBeNull();
    expect(file.opening).toBe(false);
    expect(file.progress).toBeNull();
    expect(file.error).toBeNull();
  });

  it("a second file replaces the first", async () => {
    backend.openRecordingFile.mockResolvedValueOnce(summary({ name: "a.csv" }));
    backend.openRecordingFile.mockResolvedValueOnce(summary({ name: "b.db" }));
    const file = new RecordingFile();
    await file.open();
    await file.open();
    expect(file.summary?.name).toBe("b.db");
  });

  it("keeps the open file when the dialog for the next one is dismissed", async () => {
    backend.openRecordingFile.mockResolvedValueOnce(summary({ name: "a.csv" }));
    backend.openRecordingFile.mockResolvedValueOnce(null);
    const file = new RecordingFile();
    await file.open();
    await file.open();
    expect(file.summary?.name).toBe("a.csv");
  });

  it("keeps the open file when the next one is refused", async () => {
    backend.openRecordingFile.mockResolvedValueOnce(summary({ name: "a.csv" }));
    backend.openRecordingFile.mockRejectedValueOnce(REFUSED);
    const file = new RecordingFile();
    await file.open();
    await file.open();
    expect(file.summary?.name).toBe("a.csv");
    expect(file.error).toBe(REFUSED.message);
  });

  it("does not start a second open while one runs", async () => {
    const held = pendingOpen();
    const file = new RecordingFile();
    const opening = file.open();
    await file.open();
    expect(backend.openRecordingFile).toHaveBeenCalledTimes(1);
    held.finish(summary());
    await opening;
  });

  it("Close drops the file here and in the backend", async () => {
    backend.openRecordingFile.mockResolvedValueOnce(summary());
    const file = new RecordingFile();
    await file.open();

    await file.close();

    expect(backend.closeRecordingFile).toHaveBeenCalledTimes(1);
    expect(file.summary).toBeNull();
    expect(file.error).toBeNull();
  });

  it("a failed Close keeps the file open and says why", async () => {
    backend.openRecordingFile.mockResolvedValueOnce(summary());
    const file = new RecordingFile();
    await file.open();
    backend.closeRecordingFile.mockRejectedValueOnce(REFUSED);

    await expect(file.close()).resolves.toBe(false);

    expect(file.summary).not.toBeNull();
    expect(file.error).toBe(REFUSED.message);
    await expect(file.close()).resolves.toBe(true);
    expect(file.summary).toBeNull();
    expect(file.error).toBeNull();
  });

  it("shows no Cancel or progress until the backend reports, so not over the dialog", async () => {
    const held = pendingOpen();
    const file = new RecordingFile();
    const opening = file.open();
    expect(file.opening).toBe(true);
    expect(file.working).toBe(false);

    held.report({ done: 0, total: 0 });
    expect(file.working).toBe(true);

    held.finish(summary());
    await opening;
    expect(file.working).toBe(false);
  });
});

describe("App.svelte and the recording file", () => {
  const app = readFileSync(new URL("../App.svelte", import.meta.url), "utf8");

  it("closes the backend's file once on mount: a reloaded page has no view of it", () => {
    const mount = app.slice(app.indexOf("onMount(() => {"));
    const body = mount.slice(0, mount.indexOf("\n  });"));
    expect(body.match(/recordingFile\.close\(\)/g)).toHaveLength(1);
  });

  it("leaves the File tab only when the file did close", () => {
    expect(app).toMatch(/if \(\(await recordingFile\.close\(\)\) && view === "file"\)/);
  });
});
