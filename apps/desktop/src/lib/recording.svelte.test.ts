import { describe, expect, it, vi } from "vitest";
import type {
  ExportProgress,
  ExportResult,
  RecordingEvent,
  RecordingSummary,
  UiError,
} from "./backend";
import {
  ExportJob,
  RATE_OPTIONS,
  Recording,
  dbFileName,
  defaultRecordingName,
  exportNoticeText,
  formatRecordingElapsed,
  memoryDepthText,
  rateChange,
  ratePicker,
  recordingIndicator,
  recordingOptions,
  recordingSummaryText,
  revealActionLabel,
} from "./recording.svelte";
import { DEFAULT_RECORDER_SETTINGS } from "./settings";

const CLOCK = new Date(2026, 9, 21, 9, 5, 7); // local time 2026-10-21 09:05:07

function summary(rows: number, gaps = 0, disconnected = false): RecordingSummary {
  return {
    rows,
    gaps: Array.from({ length: gaps }, (_, k) => ({
      kind: "stream" as const,
      from_ms: k * 1000,
      to_ms: k * 1000 + 500,
    })),
    disconnected,
    ended: disconnected ? "disconnected" : "stopped",
  };
}

const diskFull: UiError = {
  kind: "internal",
  message: "The disk is full; the recording stopped.",
  hint: null,
};

function deferred<T>(): { promise: Promise<T>; resolve: (v: T) => void } {
  let resolve: (v: T) => void = () => {};
  const promise = new Promise<T>((r) => (resolve = r));
  return { promise, resolve };
}

describe("rate picker (AC-0P7HSJ.2)", () => {
  it("offers 2, 10, 50 and 1000 SPS", () => {
    expect([...RATE_OPTIONS]).toEqual([2, 10, 50, 1000]);
  });

  it("is enabled while streaming and while disconnected", () => {
    expect(ratePicker("streaming", 50)).toEqual({ disabled: false, label: null });
    expect(ratePicker("disconnected", null)).toEqual({ disabled: false, label: null });
  });

  it("is disabled and explains a locked rate", () => {
    expect(ratePicker("rate_locked", 50)).toEqual({
      disabled: true,
      label: "Rate set on the meter: 50 SPS — authentication unavailable",
    });
    expect(ratePicker("rate_locked", 1000).label).toBe(
      "Rate set on the meter: 1000 SPS — authentication unavailable",
    );
  });
});

describe("recording file name (AC-0P7HSJ.3)", () => {
  it("proposes powerz-YYYYMMDD-HHMMSS.csv from the clock", () => {
    expect(defaultRecordingName(CLOCK)).toBe("powerz-20261021-090507.csv");
    expect(defaultRecordingName(new Date(2027, 0, 2, 23, 59, 58))).toBe(
      "powerz-20270102-235958.csv",
    );
  });

  it("turns a proposed CSV name into a .db name (AC-AXHK2S.6)", () => {
    expect(dbFileName("powerz-20261021-090507.csv")).toBe("powerz-20261021-090507.db");
    expect(dbFileName("bench.csv.csv")).toBe("bench.csv.db");
    expect(dbFileName("no-extension")).toBe("no-extension");
  });
});

describe("recording indicator (AC-0P7HSJ.7)", () => {
  it("formats elapsed time as h:mm:ss", () => {
    expect(formatRecordingElapsed(0)).toBe("0:00:00");
    expect(formatRecordingElapsed(65_999)).toBe("0:01:05");
    expect(formatRecordingElapsed(3_723_000)).toBe("1:02:03");
    expect(formatRecordingElapsed(36_000_000)).toBe("10:00:00");
  });

  it("shows elapsed time and row count", () => {
    const text = recordingIndicator(65_000, 1234);
    expect(text).toContain("0:01:05");
    expect(text).toMatch(/1,?234 rows/);
    expect(recordingIndicator(500, 1)).toMatch(/\b1 row\b(?!s)/);
  });
});

describe("recording summary (AC-0P7HSJ.3, AC-0P7HSJ.6)", () => {
  it("names the rows written and says there were no gaps", () => {
    const text = recordingSummaryText(summary(42));
    expect(text).toMatch(/42 rows/);
    expect(text.toLowerCase()).toContain("no gaps");
  });

  it("counts the gaps", () => {
    expect(recordingSummaryText(summary(10, 2))).toMatch(/2 gaps/);
    expect(recordingSummaryText(summary(10, 1))).toMatch(/1 gap\b/);
  });

  it("says the meter was unplugged", () => {
    const text = recordingSummaryText(summary(7, 0, true)).toLowerCase();
    expect(text).toMatch(/unplugged|disconnected/);
    expect(text).toMatch(/7 rows/);
  });
});

function deps(over: Partial<Parameters<Recording["toggle"]>[0]> = {}) {
  return {
    canRecord: true,
    startRecording: vi.fn<(name: string) => Promise<string | null>>(async () => "/tmp/rec.csv"),
    stopRecording: vi.fn(async (): Promise<RecordingSummary> => summary(5)),
    now: () => CLOCK,
    ...over,
  };
}

describe("Recording state (AC-0P7HSJ.3, AC-0P7HSJ.6, AC-0P7HSJ.7)", () => {
  it("starts with the proposed file name, then stops with a summary", async () => {
    const r = new Recording();
    const d = deps();
    expect(r.phase).toBe("idle");
    await r.toggle(d);
    expect(d.startRecording).toHaveBeenCalledWith("powerz-20261021-090507.csv");
    expect(r.phase).toBe("recording");
    expect(r.path).toBe("/tmp/rec.csv");
    expect(r.indicator).toContain("0:00:00");

    r.handleEvent({ state: "progress", rows: 120, elapsed_ms: 2_400, paused: false });
    expect(r.rows).toBe(120);
    expect(r.indicator).toContain("0:00:02");
    expect(r.indicator).toMatch(/120 rows/);

    await r.toggle(d);
    expect(d.stopRecording).toHaveBeenCalledTimes(1);
    expect(r.phase).toBe("idle");
    expect(r.indicator).toBeNull();
    expect(r.notice).toMatch(/5 rows/);
  });

  it("does nothing when there is no stream to record", async () => {
    const r = new Recording();
    const d = deps({ canRecord: false });
    await r.toggle(d);
    expect(d.startRecording).not.toHaveBeenCalled();
    expect(r.phase).toBe("idle");
  });

  it("stays idle when the save dialog is cancelled", async () => {
    const r = new Recording();
    await r.toggle(deps({ startRecording: vi.fn(async () => null) }));
    expect(r.phase).toBe("idle");
    expect(r.indicator).toBeNull();
    expect(r.error).toBeNull();
  });

  it("shows a refused start as an error", async () => {
    const r = new Recording();
    await r.toggle(deps({ startRecording: vi.fn(async () => Promise.reject(diskFull)) }));
    expect(r.phase).toBe("idle");
    expect(r.error).toEqual(diskFull);
  });

  it("ignores a second press while the dialog is open", async () => {
    const r = new Recording();
    const pending = deferred<string | null>();
    const d = deps({ startRecording: vi.fn(() => pending.promise) });
    const first = r.toggle(d);
    await r.toggle(d);
    expect(d.startRecording).toHaveBeenCalledTimes(1);
    pending.resolve("/tmp/a.csv");
    await first;
    expect(r.phase).toBe("recording");
  });

  it("ends with a notice when the meter is unplugged", async () => {
    const r = new Recording();
    await r.toggle(deps());
    r.handleEvent({ state: "stopped", ...summary(300, 0, true) });
    expect(r.phase).toBe("idle");
    expect(r.notice?.toLowerCase()).toMatch(/unplugged|disconnected/);
    expect(r.notice).toMatch(/300 rows/);
  });

  it("ends with the error and the kept rows on a write failure", async () => {
    const r = new Recording();
    await r.toggle(deps());
    const failed: RecordingEvent = { state: "failed", rows: 812, error: diskFull };
    r.handleEvent(failed);
    expect(r.phase).toBe("idle");
    expect(r.error).toEqual(diskFull);
    expect(r.notice).toMatch(/812 rows/);
  });
});

describe("Recording.stop (AC-4PJRVJ.2)", () => {
  it("says when the stop failed", async () => {
    const r = new Recording();
    await r.toggle(deps());
    const failed = deps({ stopRecording: vi.fn(() => Promise.reject(new Error("no"))) });
    expect(await r.stop(failed)).toBe(false);
    expect(r.error).not.toBeNull();
  });

  it("starts nothing when the recording has already ended", async () => {
    const r = new Recording();
    const d = deps();
    expect(await r.stop(d)).toBe(true);
    expect(d.startRecording).not.toHaveBeenCalled();
    expect(r.phase).toBe("idle");
  });
});

describe("A start that timed out and started anyway (AC-AXHK2K.9)", () => {
  it("shows as recording, with the error gone, and can be stopped", async () => {
    const r = new Recording();
    const timeout = { kind: "timeout", message: "The meter is busy. Try again.", hint: null };
    const d = deps({ startRecording: vi.fn(() => Promise.reject(timeout)) });
    await r.toggle(d);
    expect(r.phase).toBe("idle");
    expect(r.error?.kind).toBe("timeout");

    r.handleEvent({ state: "started" });

    expect(r.phase).toBe("recording");
    expect(r.error).toBeNull();
    await r.stop(d);
    expect(d.stopRecording).toHaveBeenCalledTimes(1);
  });

  it("changes nothing for the usual start, whose answer is on its way", async () => {
    const r = new Recording();
    let started: (path: string | null) => void = () => {};
    const d = deps({
      startRecording: vi.fn(() => new Promise<string | null>((res) => (started = res))),
    });
    const starting = r.toggle(d);
    r.handleEvent({ state: "started" });
    expect(r.phase).toBe("starting");
    started("/tmp/rec.csv");
    await starting;
    expect(r.path).toBe("/tmp/rec.csv");
  });
});

describe("Recording.stop while it cannot stop yet (AC-AXHK2F.7)", () => {
  it("answers false while a start is in flight, and the start goes on", async () => {
    const r = new Recording();
    let started: (path: string | null) => void = () => {};
    const d = deps({
      startRecording: vi.fn(() => new Promise<string | null>((res) => (started = res))),
    });
    const starting = r.toggle(d);
    expect(r.phase).toBe("starting");

    expect(await r.stop(d)).toBe(false);
    expect(d.stopRecording).not.toHaveBeenCalled();

    started("/tmp/rec.csv");
    await starting;
    expect(r.phase).toBe("recording");
  });

  it("answers false while a stop is in flight, and that stop goes on", async () => {
    const r = new Recording();
    let stopped: (s: RecordingSummary) => void = () => {};
    const d = deps({
      stopRecording: vi.fn(() => new Promise<RecordingSummary>((res) => (stopped = res))),
    });
    await r.toggle(d);
    const stopping = r.stop(d);
    expect(r.phase).toBe("stopping");

    expect(await r.stop(d)).toBe(false);
    expect(d.stopRecording).toHaveBeenCalledTimes(1);

    stopped(summary(5));
    expect(await stopping).toBe(true);
    expect(r.phase).toBe("idle");
  });

  it("a failed stop does not mark the recording ended", async () => {
    const r = new Recording();
    await r.toggle(deps());
    const failed = deps({ stopRecording: vi.fn(() => Promise.reject(diskFull)) });

    expect(await r.stop(failed)).toBe(false);

    expect(r.phase).toBe("recording");
    expect(r.path).toBe("/tmp/rec.csv");
    expect(r.indicator).toMatch(/^REC /);
    expect(r.notice).toBeNull();
    expect(r.error).toEqual(diskFull);
  });

  it("a stop that failed can be tried again", async () => {
    const r = new Recording();
    await r.toggle(deps());
    await r.stop(deps({ stopRecording: vi.fn(() => Promise.reject(diskFull)) }));

    expect(await r.stop(deps())).toBe(true);
    expect(r.phase).toBe("idle");
    expect(r.notice).toMatch(/5 rows/);
  });
});

describe("Recording.stop retried after a failure (AC-AXHK2F.7)", () => {
  it("clears the earlier error when the retry succeeds", async () => {
    const r = new Recording();
    await r.toggle(deps());
    await r.stop(deps({ stopRecording: vi.fn(() => Promise.reject(diskFull)) }));
    expect(r.error).toEqual(diskFull);

    expect(await r.stop(deps())).toBe(true);

    expect(r.error).toBeNull();
    expect(r.phase).toBe("idle");
  });
});

describe("Recording.stop whose last flush fails (AC-AXHK2F.7)", () => {
  // The backend has dropped the recording by then: it sends `failed` and rejects the stop,
  // and either can reach the page first.
  const lost: RecordingEvent = { state: "failed", rows: 4, error: diskFull };
  const nothing: UiError = { kind: "usage", message: "Nothing is being recorded.", hint: null };

  async function stoppedLosing(order: "event first" | "rejection first"): Promise<{
    r: Recording;
    answer: boolean;
  }> {
    const r = new Recording();
    await r.toggle(deps());
    let fail: (e: unknown) => void = () => {};
    const stopping = r.stop(
      deps({ stopRecording: vi.fn(() => new Promise<RecordingSummary>((_, rej) => (fail = rej))) }),
    );
    if (order === "event first") r.handleEvent(lost);
    fail(diskFull);
    const answer = await stopping;
    if (order === "rejection first") r.handleEvent(lost);
    return { r, answer };
  }

  it.each(["event first", "rejection first"] as const)(
    "answers false and shows no running recording (%s)",
    async (order) => {
      const { r, answer } = await stoppedLosing(order);

      expect(answer).toBe(false);
      expect(r.phase).toBe("idle");
      expect(r.indicator).toBeNull();
      expect(r.error).toEqual(diskFull);
      expect(r.notice).toMatch(/4 rows/);
      expect(r.savedPath).toBe("/tmp/rec.csv");
    },
  );

  it.each(["event first", "rejection first"] as const)(
    "a retry asks the backend nothing and keeps the real reason (%s)",
    async (order) => {
      const { r } = await stoppedLosing(order);
      const retry = deps({ stopRecording: vi.fn(() => Promise.reject(nothing)) });

      expect(await r.stop(retry)).toBe(true);

      expect(retry.stopRecording).not.toHaveBeenCalled();
      expect(r.error).toEqual(diskFull);
    },
  );
});

describe("ExportJob (AC-0P7HSJ.5)", () => {
  it("proposes a file name, tracks progress and reports the rows", async () => {
    const job = new ExportJob();
    const pending = deferred<ExportResult | null>();
    let report: (p: ExportProgress) => void = () => {};
    const exportSession = vi.fn((_name: string, onProgress: (p: ExportProgress) => void) => {
      report = onProgress;
      return pending.promise;
    });
    const run = job.run({ exportSession, now: () => CLOCK });
    expect(exportSession).toHaveBeenCalledWith("powerz-20261021-090507.csv", expect.any(Function));
    expect(job.running).toBe(true);
    report({ done: 250, total: 1000 });
    expect(job.fraction).toBeCloseTo(0.25);
    // A second export while one runs is ignored.
    await job.run({ exportSession, now: () => CLOCK });
    expect(exportSession).toHaveBeenCalledTimes(1);

    pending.resolve({ path: "/tmp/x.csv", rows: 1000 });
    await run;
    expect(job.running).toBe(false);
    expect(job.rows).toBe(1000);
    expect(job.error).toBeNull();
  });

  it("is a no-op when the dialog is cancelled and shows a failure", async () => {
    const job = new ExportJob();
    await job.run({ exportSession: vi.fn(async () => null), now: () => CLOCK });
    expect(job.running).toBe(false);
    expect(job.rows).toBeNull();

    await job.run({ exportSession: vi.fn(async () => Promise.reject(diskFull)), now: () => CLOCK });
    expect(job.running).toBe(false);
    expect(job.error).toEqual(diskFull);
  });
});

describe("rate picker before the locked rate is measured (review)", () => {
  it("says the rate is being measured instead of showing a placeholder number", () => {
    expect(ratePicker("rate_locked", null)).toEqual({
      disabled: true,
      label: "Rate set on the meter (measuring…) — authentication unavailable",
    });
  });
});

describe("rate picker while recording (review)", () => {
  it("is disabled while a recording is open, so the file never mixes rates", () => {
    expect(ratePicker("streaming", 50, true)).toEqual({
      disabled: true,
      label: "Stop the recording to change the rate",
    });
    expect(ratePicker("streaming", 50, false)).toEqual({ disabled: false, label: null });
  });
});

describe("Recording after the session ends underneath it (review)", () => {
  it("returns to idle when the session disconnects without a recording event", async () => {
    const rec = new Recording();
    await rec.toggle(deps());
    expect(rec.phase).toBe("recording");
    rec.sessionEnded();
    expect(rec.phase).toBe("idle");
    expect(rec.notice).not.toBeNull();
  });

  it("shows no stop error when an unplug already ended the recording", async () => {
    const rec = new Recording();
    await rec.toggle(deps());
    let fail: (e: unknown) => void = () => {};
    const stopping = rec.toggle(
      deps({ stopRecording: vi.fn(() => new Promise<RecordingSummary>((_, rej) => (fail = rej))) }),
    );
    rec.handleEvent({ state: "stopped", ...summary(5, 0, true) });
    fail({ kind: "usage", message: "The meter is not connected.", hint: null });
    await stopping;
    expect(rec.error).toBeNull();
    expect(rec.notice).toContain("unplugged");
    expect(rec.phase).toBe("idle");
  });
});

describe("Recording races with the session (review round 2)", () => {
  it("does not show a recording that resolves after the session ended", async () => {
    const rec = new Recording();
    let resolve: (p: string | null) => void = () => {};
    const starting = rec.toggle(
      deps({ startRecording: vi.fn(() => new Promise<string | null>((r) => (resolve = r))) }),
    );
    rec.sessionEnded();
    resolve("/tmp/late.csv");
    await starting;
    expect(rec.phase).toBe("idle");
  });
});

// ---- FR VEDDFD: a recording that kept running through a reload ----

describe("Recording adopts a running recording on load (AC-VEDDFD.3, AC-VEDDFD.5)", () => {
  it("shows it as recording with the backend's elapsed time and rows", () => {
    const r = new Recording();
    r.adopt({ rows: 1_234, elapsed_ms: 65_000 });
    expect(r.phase).toBe("recording");
    expect(r.rows).toBe(1_234);
    expect(r.elapsedMs).toBe(65_000);
    expect(r.indicator).toBe("REC 0:01:05 · 1,234 rows");
  });

  it("follows the progress that comes after", () => {
    const r = new Recording();
    r.adopt({ rows: 100, elapsed_ms: 2_000 });
    r.handleEvent({ state: "progress", rows: 125, elapsed_ms: 2_500, paused: false });
    expect(r.indicator).toBe("REC 0:00:02 · 125 rows");
  });

  it("stops through the usual toggle", async () => {
    const r = new Recording();
    r.adopt({ rows: 100, elapsed_ms: 2_000 });
    const d = deps();
    await r.toggle(d);
    expect(d.stopRecording).toHaveBeenCalledTimes(1);
    expect(d.startRecording).not.toHaveBeenCalled();
    expect(r.phase).toBe("idle");
    expect(r.notice).toBe(recordingSummaryText(summary(5)));
  });

  it("ends with the session like one started on this page", () => {
    const r = new Recording();
    r.adopt({ rows: 100, elapsed_ms: 2_000 });
    r.handleEvent({ state: "stopped", ...summary(140, 0, true) });
    expect(r.phase).toBe("idle");
    expect(r.indicator).toBeNull();
  });
});

describe("rate keys while recording (AC-JFD9XR.2)", () => {
  it("refuses a rate change while a recording is open, with the picker's notice", () => {
    for (const sps of [2, 10, 50, 1000]) {
      expect(rateChange(sps, { recording: true })).toEqual({
        kind: "refused",
        notice: "Stop the recording to change the rate.",
      });
    }
  });

  it("lets the rate change through when nothing is recording", () => {
    expect(rateChange(1000, { recording: false })).toEqual({ kind: "set", sps: 1000 });
    expect(rateChange(2, { recording: false })).toEqual({ kind: "set", sps: 2 });
  });
});

describe("export notice (AC-9CFMD3.1)", () => {
  it("names the rows written and the file path", () => {
    const text = exportNoticeText({ rows: 1234, path: "/Users/me/powerz-20261021-090507.csv" });
    expect(text).toContain("1,234 rows");
    expect(text).toContain("/Users/me/powerz-20261021-090507.csv");
  });

  it("says 1 row for a single row", () => {
    expect(exportNoticeText({ rows: 1, path: "C:\\data\\x.csv" })).toMatch(/\b1 row\b/);
    expect(exportNoticeText({ rows: 1, path: "C:\\data\\x.csv" })).toContain("C:\\data\\x.csv");
  });

  it("keeps the rows and path of a finished export for the notice", async () => {
    const job = new ExportJob();
    await job.run({
      exportSession: vi.fn(async () => ({ path: "/tmp/export.csv", rows: 42 })),
      now: () => CLOCK,
    });
    expect(job.rows).toBe(42);
    expect(job.path).toBe("/tmp/export.csv");
  });
});

describe("reveal action label (AC-9CFMD3.1)", () => {
  it('reads "Reveal in Finder" on macOS', () => {
    expect(revealActionLabel("MacIntel")).toBe("Reveal in Finder");
    expect(revealActionLabel("macOS")).toBe("Reveal in Finder");
  });

  it('reads "Show in folder" on Windows and Linux', () => {
    expect(revealActionLabel("Win32")).toBe("Show in folder");
    expect(revealActionLabel("Windows")).toBe("Show in folder");
    expect(revealActionLabel("Linux x86_64")).toBe("Show in folder");
  });
});

describe("recording saved notice (AC-9CFMD3.2)", () => {
  it("gives the rows, the gaps and the file path", () => {
    const text = recordingSummaryText(summary(1500, 2), "/tmp/rec.csv");
    expect(text).toContain("1,500 rows");
    expect(text).toContain("2 gaps");
    expect(text).toContain("/tmp/rec.csv");
  });

  it("keeps the saved file's path after a stop, for the reveal and copy actions", async () => {
    const r = new Recording();
    const d = deps();
    await r.toggle(d);
    await r.toggle(d);
    expect(r.phase).toBe("idle");
    expect(r.savedPath).toBe("/tmp/rec.csv");
    expect(r.notice).toContain("/tmp/rec.csv");
    expect(r.notice).toMatch(/5 rows/);
  });

  it("keeps the path when an unplug ends the recording", async () => {
    const r = new Recording();
    await r.toggle(deps());
    r.handleEvent({ state: "stopped", ...summary(300, 1, true) });
    expect(r.savedPath).toBe("/tmp/rec.csv");
    expect(r.notice).toContain("/tmp/rec.csv");
    expect(r.notice).toMatch(/1 gap\b/);
  });

  it("forgets the path when the notice is dismissed or a new recording starts", async () => {
    const r = new Recording();
    const d = deps();
    await r.toggle(d);
    await r.toggle(d);
    expect(r.savedPath).toBe("/tmp/rec.csv");
    r.dismiss();
    expect(r.savedPath).toBeNull();

    await r.toggle(d);
    await r.toggle(d);
    expect(r.savedPath).toBe("/tmp/rec.csv");
    await r.toggle(d);
    expect(r.phase).toBe("recording");
    expect(r.savedPath).toBeNull();
  });
});

// ---- FR AXHK2R: pause, auto-stop, limits, prefix and folder ----

describe("the file name's prefix (AC-AXHK2R.6)", () => {
  it("replaces powerz", () => {
    expect(defaultRecordingName(CLOCK, "bench")).toBe("bench-20261021-090507.csv");
  });

  it("means powerz when empty", () => {
    expect(defaultRecordingName(CLOCK, "")).toBe("powerz-20261021-090507.csv");
    expect(defaultRecordingName(CLOCK)).toBe("powerz-20261021-090507.csv");
  });
});

describe("the options sent to the backend (AC-AXHK2R.3, .5, .6)", () => {
  it("sends no folder and no auto-stop by default", () => {
    const options = recordingOptions(DEFAULT_RECORDER_SETTINGS);
    expect(options.folder ?? null).toBeNull();
    expect(options.below_a ?? null).toBeNull();
    expect(options.for_ms ?? null).toBeNull();
  });

  it("sends the folder and the row limit", () => {
    const options = recordingOptions({
      ...DEFAULT_RECORDER_SETTINGS,
      folder: "/data/runs",
      maxRows: 5_000,
    });
    expect(options.folder).toBe("/data/runs");
    expect(options.max_rows).toBe(5_000);
  });

  it("sends the wait in ms while auto-stop is on", () => {
    const options = recordingOptions({
      ...DEFAULT_RECORDER_SETTINGS,
      autoStop: true,
      belowA: 0.05,
      forSeconds: 30,
    });
    expect(options.below_a).toBe(0.05);
    expect(options.for_ms).toBe(30_000);
  });

  it("sends neither value while auto-stop is off, whatever the fields hold", () => {
    const options = recordingOptions({
      ...DEFAULT_RECORDER_SETTINGS,
      autoStop: false,
      belowA: 0.05,
      forSeconds: 30,
    });
    expect(options.below_a ?? null).toBeNull();
    expect(options.for_ms ?? null).toBeNull();
  });
});

describe("memory depth (AC-AXHK2R.7)", () => {
  it("says how long the chart's memory lasts, in plain words", () => {
    expect(memoryDepthText(600_000, 1000)).toMatch(/\b10 minutes\b/);
    expect(memoryDepthText(600_000, 50)).toMatch(/\b3(\.\d)? hours?\b/);
    expect(memoryDepthText(600_000, 2)).toMatch(/\b(3 days?|83 hours)\b/);
    expect(memoryDepthText(30_000, 1000)).toMatch(/\b30 seconds\b/);
    expect(memoryDepthText(120, 2)).toMatch(/\b1 minute\b/);
  });

  it("follows the rate", () => {
    expect(memoryDepthText(600_000, 50)).not.toBe(memoryDepthText(600_000, 1000));
  });

  it('reads "measuring" until the rate is known', () => {
    expect(memoryDepthText(600_000, null)).toMatch(/measuring/i);
    expect(memoryDepthText(600_000, 0)).toMatch(/measuring/i);
  });
});

describe("the summary says why a recording ended (AC-AXHK2R.3, AC-AXHK2R.5)", () => {
  it("names the low current", () => {
    const text = recordingSummaryText({ ...summary(42), ended: "low_current" }, "/tmp/rec.csv");
    expect(text.toLowerCase()).toContain("low current");
    expect(text).toContain("42 rows");
    expect(text).toContain("/tmp/rec.csv");
  });

  it("names the row limit", () => {
    const text = recordingSummaryText({ ...summary(1000), ended: "row_limit" }, "/tmp/rec.csv");
    expect(text.toLowerCase()).toContain("row limit");
    expect(text).toContain("1,000 rows");
    expect(text).toContain("/tmp/rec.csv");
  });

  it("says nothing new for a stop the user asked for", () => {
    const text = recordingSummaryText(summary(5)).toLowerCase();
    expect(text).not.toContain("low current");
    expect(text).not.toContain("row limit");
  });

  it("counts a pause as a gap", () => {
    const paused: RecordingSummary = {
      ...summary(10),
      gaps: [{ kind: "pause", from_ms: 100, to_ms: 2_000 }],
    };
    expect(recordingSummaryText(paused)).toMatch(/1 gap\b/);
  });

  it("ends the recording and keeps its file when a rule stops it", async () => {
    const r = new Recording();
    await r.toggle(deps());
    r.handleEvent({ state: "stopped", ...summary(300), ended: "row_limit" });
    expect(r.phase).toBe("idle");
    expect(r.savedPath).toBe("/tmp/rec.csv");
    expect(r.notice?.toLowerCase()).toContain("row limit");
  });
});

function pauseDeps() {
  return {
    pauseRecording: vi.fn(async (): Promise<void> => {}),
    resumeRecording: vi.fn(async (): Promise<void> => {}),
  };
}

describe("Recording.togglePause (AC-AXHK2R.1)", () => {
  it("pauses, then resumes", async () => {
    const r = new Recording();
    await r.toggle(deps());
    const d = pauseDeps();
    expect(r.paused).toBe(false);

    await r.togglePause(d);
    expect(d.pauseRecording).toHaveBeenCalledTimes(1);
    expect(r.paused).toBe(true);
    expect(r.phase).toBe("recording");

    await r.togglePause(d);
    expect(d.resumeRecording).toHaveBeenCalledTimes(1);
    expect(r.paused).toBe(false);
  });

  it("does nothing without a running recording", async () => {
    const r = new Recording();
    const d = pauseDeps();
    await r.togglePause(d);
    expect(d.pauseRecording).not.toHaveBeenCalled();
    expect(r.paused).toBe(false);
  });

  it("shows a refused pause as an error and stays as it was", async () => {
    const r = new Recording();
    await r.toggle(deps());
    await r.togglePause({ ...pauseDeps(), pauseRecording: vi.fn(() => Promise.reject(diskFull)) });
    expect(r.paused).toBe(false);
    expect(r.error).toEqual(diskFull);
    expect(r.phase).toBe("recording");
  });

  it("follows the backend's progress", async () => {
    const r = new Recording();
    await r.toggle(deps());
    r.handleEvent({ state: "progress", rows: 5, elapsed_ms: 500, paused: true });
    expect(r.paused).toBe(true);
    r.handleEvent({ state: "progress", rows: 5, elapsed_ms: 1_000, paused: false });
    expect(r.paused).toBe(false);
  });

  it("is over when the recording is", async () => {
    const stopped = new Recording();
    const d = deps();
    await stopped.toggle(d);
    await stopped.togglePause(pauseDeps());
    await stopped.toggle(d);
    expect(stopped.paused).toBe(false);

    const ruled = new Recording();
    await ruled.toggle(deps());
    await ruled.togglePause(pauseDeps());
    ruled.handleEvent({ state: "stopped", ...summary(5), ended: "row_limit" });
    expect(ruled.paused).toBe(false);
  });

  it("carries across a reload and a move between views", async () => {
    const r = new Recording();
    r.adopt({ rows: 10, elapsed_ms: 1_000, paused: true });
    expect(r.paused).toBe(true);
    expect(r.detach()).toMatchObject({ rows: 10, paused: true });
    expect(r.paused).toBe(false);
  });
});

describe("Recording races with a stop rule (review)", () => {
  const nothing = { kind: "usage", message: "Nothing is being recorded.", hint: null };
  const ended: RecordingEvent = {
    state: "stopped",
    rows: 9,
    gaps: [],
    disconnected: false,
    ended: "low_current",
  };

  async function recordingNow(): Promise<Recording> {
    const r = new Recording();
    await r.toggle({
      canRecord: true,
      startRecording: () => Promise.resolve("/data/a.csv"),
      stopRecording: () => Promise.reject(nothing),
      now: () => new Date(2026, 9, 21),
    });
    return r;
  }

  it("a Stop the rule beat leaves no error beside the rule's notice", async () => {
    const r = await recordingNow();
    await r.stop({
      canRecord: true,
      startRecording: () => Promise.resolve(null),
      stopRecording: () => Promise.reject(nothing),
      now: () => new Date(),
    });
    expect(r.error?.message).toBe(nothing.message);
    r.handleEvent(ended);
    expect(r.error).toBeNull();
    expect(r.notice).toContain("/data/a.csv");
    expect(r.phase).toBe("idle");
  });

  it("a Pause the rule beat leaves no error beside the rule's notice", async () => {
    const r = await recordingNow();
    await r.togglePause({
      pauseRecording: () => Promise.reject(nothing),
      resumeRecording: () => Promise.resolve(),
    });
    r.handleEvent(ended);
    expect(r.error).toBeNull();
  });

  it("an end before the start answered still names the file", async () => {
    const r = new Recording();
    let answer: (path: string) => void = () => undefined;
    const started = r.toggle({
      canRecord: true,
      startRecording: () => new Promise((resolve) => (answer = resolve)),
      stopRecording: () => Promise.reject(nothing),
      now: () => new Date(2026, 9, 21),
    });
    r.handleEvent(ended);
    answer("/data/b.csv");
    await started;
    expect(r.phase).toBe("idle");
    expect(r.savedPath).toBe("/data/b.csv");
    expect(r.notice).toContain("/data/b.csv");
  });

  it("a progress sent before a pause does not undo the press on its way", async () => {
    const r = await recordingNow();
    let answer: () => void = () => undefined;
    const pausing = r.togglePause({
      pauseRecording: () => new Promise((resolve) => (answer = resolve)),
      resumeRecording: () => Promise.resolve(),
    });
    r.handleEvent({ state: "progress", rows: 2, elapsed_ms: 500, paused: false });
    r.handleEvent({ state: "progress", rows: 2, elapsed_ms: 500, paused: true });
    expect(r.paused).toBe(false);
    answer();
    await pausing;
    expect(r.paused).toBe(true);
  });

  it("a second press waits for the first", async () => {
    const r = await recordingNow();
    const pauseRecording = vi.fn(() => Promise.resolve());
    const resumeRecording = vi.fn(() => Promise.resolve());
    const first = r.togglePause({ pauseRecording, resumeRecording });
    await r.togglePause({ pauseRecording, resumeRecording });
    await first;
    expect(pauseRecording).toHaveBeenCalledTimes(1);
    expect(resumeRecording).not.toHaveBeenCalled();
  });
});
