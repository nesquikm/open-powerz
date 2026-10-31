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
  defaultRecordingName,
  exportNoticeText,
  formatRecordingElapsed,
  rateChange,
  ratePicker,
  recordingIndicator,
  recordingSummaryText,
  revealActionLabel,
} from "./recording.svelte";

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

    r.handleEvent({ state: "progress", rows: 120, elapsed_ms: 2_400 });
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
    rec.handleEvent({ state: "stopped", rows: 5, gaps: [], disconnected: true });
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
    r.handleEvent({ state: "progress", rows: 125, elapsed_ms: 2_500 });
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
    r.handleEvent({ state: "stopped", rows: 140, gaps: [], disconnected: true });
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
