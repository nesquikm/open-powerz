/**
 * FR AXHK2W (AC-AXHK2W.14): exporting a session whose history has folded writes only its
 * full-resolution part, and the app says when the export finishes how much of the oldest
 * part was folded and left out and the time the file starts at.
 *
 * API pinned here, beyond the FR's Technical Design:
 *
 * - `ExportFolded { samples: number; ms: number; start_unix_ms: number }` exported from
 *   `backend.ts`; `ExportResult.folded?: ExportFolded | null` (absent or `null` when nothing
 *   was left out), as the Tauri commands `export_session` / `export_session_db` answer it;
 * - `exportNoticeText(result)` keeps its rows-and-path text and, when `folded` is set, also
 *   says the oldest `formatRecordingElapsed(folded.ms)` (`samples` samples, `en-US` grouped)
 *   was folded and left out, and that the file starts at `YYYY-MM-DD HH:MM:SS` local time of
 *   `start_unix_ms`;
 * - `ExportJob.folded: ExportFolded | null`: the last finished export's `folded`
 *   (`null` when none, cleared with the notice);
 * - the demo backend's `exportSession` (and `exportSessionDb`) answer `folded` once its history
 *   is past `MockOptions.historyFullSamples`: the samples before `historyFoldBoundary()`, the
 *   span from the session's first sample to it, and a start time; `null` otherwise.
 */

import { afterEach, describe, expect, it, vi } from "vitest";

import type { ExportFolded, ExportResult } from "./backend";
import { MockBackend } from "./mockBackend";
import { ExportJob, exportNoticeText, formatRecordingElapsed } from "./recording.svelte";

const pad = (n: number): string => String(n).padStart(2, "0");
const localStamp = (ms: number): string => {
  const d = new Date(ms);
  return (
    `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ` +
    `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`
  );
};

// A synthetic start: 2026-10-21 09:05:07 local time.
const START = new Date(2026, 9, 21, 9, 5, 7).getTime();
const FOLDED: ExportFolded = {
  samples: 98_000_000,
  ms: 27 * 3600_000 + 13 * 60_000,
  start_unix_ms: START,
};

describe("export notice of a folded session (AC-AXHK2W.14)", () => {
  it("keeps the rows and the file and says what was folded and left out", () => {
    const text = exportNoticeText({ rows: 10_000_000, path: "/synthetic/x.csv", folded: FOLDED });
    expect(text).toContain("10,000,000 rows");
    expect(text).toContain("/synthetic/x.csv");
    expect(text).toMatch(/folded/i);
    expect(text).toMatch(/left out/i);
    expect(text).toContain(formatRecordingElapsed(FOLDED.ms));
    expect(text).toContain("98,000,000 samples");
  });

  it("names the time the file starts at", () => {
    const text = exportNoticeText({ rows: 2_000, path: "/synthetic/x.csv", folded: FOLDED });
    expect(text).toContain(localStamp(START));
  });

  it("says nothing about folding when nothing was left out", () => {
    for (const result of [
      { rows: 5, path: "/synthetic/y.csv" },
      { rows: 5, path: "/synthetic/y.csv", folded: null },
    ] satisfies ExportResult[]) {
      const text = exportNoticeText(result);
      expect(text).not.toMatch(/folded|left out/i);
      expect(text).toBe("Exported 5 rows to /synthetic/y.csv");
    }
  });

  it("keeps what was folded for the notice, and clears it with the notice", async () => {
    const job = new ExportJob();
    await job.run({
      exportSession: vi.fn(async () => ({ path: "/synthetic/x.csv", rows: 2_000, folded: FOLDED })),
      now: () => new Date(START),
    });
    expect(job.rows).toBe(2_000);
    expect(job.folded).toEqual(FOLDED);
    job.dismissNotice();
    expect(job.folded).toBeNull();

    await job.run({
      exportSession: vi.fn(async () => ({ path: "/synthetic/y.csv", rows: 3 })),
      now: () => new Date(START),
    });
    expect(job.folded).toBeNull();
  });
});

let backend: MockBackend | null = null;
afterEach(async () => {
  await backend?.disconnect().catch(() => {});
  backend = null;
  vi.useRealTimers();
});

/** The demo stream's batch tick: 25 samples at 1000 samples per second. */
const STREAM_TICK_MS = 25;

/** Runs a demo export to its end on fake timers: only the export's own zero-delay timer
 * fires, never a stream tick, so the history holds still (FR B67EYP). */
async function settled<T>(pending: Promise<T>): Promise<T> {
  await vi.advanceTimersByTimeAsync(0);
  return pending;
}

describe("demo backend export of a folded session (AC-AXHK2W.14)", () => {
  it("reports nothing folded under its limit", async () => {
    backend = new MockBackend(null, { historyFullSamples: 5_000 });
    await backend.connect("demo:load-5v-1a", 1000, () => {});
    await backend.seedSyntheticHistory(4_000);
    const result = await backend.exportSession("synthetic.csv", () => {});
    expect(result?.folded ?? null).toBeNull();
  });

  it("reports the folded samples, their span and the file's start past its limit", async () => {
    // FR B67EYP: the stream's timers are fake, so no demo sample arrives between the
    // boundary read and either export, and the checks below stay exact.
    vi.useFakeTimers();
    backend = new MockBackend(null, { historyFullSamples: 5_000 });
    await backend.connect("demo:load-5v-1a", 1000, () => {});
    // One sample per ms from 0.
    await backend.seedSyntheticHistory(20_000);
    const boundary = await backend.historyFoldBoundary();
    expect(boundary).not.toBeNull();
    for (const exportFn of [
      (b: MockBackend) => b.exportSession("synthetic.csv", () => {}),
      (b: MockBackend) => b.exportSessionDb("synthetic.db", () => {}),
    ]) {
      const result = await settled(exportFn(backend));
      const folded = result?.folded;
      expect(folded).toBeTruthy();
      expect(folded?.samples).toBe(boundary);
      expect(folded?.ms).toBe(boundary);
      expect(typeof folded?.start_unix_ms).toBe("number");
      expect(folded?.start_unix_ms).toBeGreaterThan(0);
    }
  });

  // FR B67EYP (AC-B67EYP.2): the hazard the test above shuts out. One stream tick between
  // the boundary read and an export grows the history, so the export's fold no longer
  // matches the boundary read before it: the CI failure `expected 15025 to be 15000`.
  it("drifts from the boundary read when a stream tick lands before the export", async () => {
    vi.useFakeTimers();
    backend = new MockBackend(null, { historyFullSamples: 5_000 });
    await backend.connect("demo:load-5v-1a", 1000, () => {});
    await backend.seedSyntheticHistory(20_000);
    const boundary = await backend.historyFoldBoundary();
    expect(boundary).toBe(15_000);
    await vi.advanceTimersByTimeAsync(STREAM_TICK_MS);
    const result = await settled(backend.exportSession("synthetic.csv", () => {}));
    expect(result?.folded?.samples).toBe(15_025);
    expect(result?.folded?.samples).not.toBe(boundary);
    expect(await backend.historyFoldBoundary()).toBe(15_025);
  });
});
