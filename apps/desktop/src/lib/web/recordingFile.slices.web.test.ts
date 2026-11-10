/**
 * FR 3B70S6: Cancel interrupts a large Open recording read in the browser.
 *
 * - AC-3B70S6.1: a CSV is read in slices of at most `RECORDING_SLICE_LINES` (5,000)
 *   lines through the wasm `CsvRecording`, a `.db` in slices of at most 5,000 rows,
 *   with a yield to the event loop (a macrotask) between slices.
 * - AC-3B70S6.2: a Cancel during the read stops it before the next slice: the open
 *   resolves `null`, no further slice is read and the wasm reader is freed.
 * - AC-3B70S6.3: after a cancelled open the file open before it still answers the
 *   range, statistics and close commands.
 * - AC-3B70S6.4: an open that is not cancelled gives the one-shot reader's rows,
 *   skipped and left-out counts, statistics and progress.
 *
 * The real wasm module runs underneath; its `CsvRecording` and `DbRecording` are
 * wrapped to count slices, frees and finishes.
 */

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import initSqlJs, { type SqlJsStatic } from "sql.js";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import type { FileSummary, OpenProgress, UiError } from "../backend";
import type { PowerzWasm, RecordingRows } from "./driver";
import {
  RECORDING_SLICE_LINES,
  RecordingFiles,
  openedFromRows,
  wasmRecordingReader,
  type PickedRecordingFile,
} from "./recordingFile";
import { loadWasm } from "./testWasm.test";

const SQL_DIST = new URL("../../../node_modules/sql.js/dist/", import.meta.url);
const DATA = new URL("../../../src-tauri/tests/data/", import.meta.url);

let w: PowerzWasm;
let SQL: SqlJsStatic;
const loadSqlJs = (): Promise<SqlJsStatic> =>
  initSqlJs({ locateFile: (f: string) => fileURLToPath(new URL(f, SQL_DIST)) });

/** A macrotask probe: fired once either a `setTimeout(0)` or a `MessageChannel` message
 * queued at arming time has run, whichever kind of yield the reader uses. */
const channel = new MessageChannel();
let probeFired = true;
let probeArmed = false;
channel.port1.onmessage = () => {
  probeFired = true;
};
function armProbe(): void {
  probeFired = false;
  probeArmed = true;
  setTimeout(() => {
    probeFired = true;
  }, 0);
  channel.port2.postMessage(0);
}

beforeAll(async () => {
  w = await loadWasm();
  SQL = await loadSqlJs();
});

afterAll(() => {
  channel.port1.close();
  channel.port2.close();
});

// ---- the wrapped wasm module ----

interface Trace {
  csvCreated: number;
  steps: number[];
  /** Whether a macrotask ran between the previous step and this one. */
  stepYielded: boolean[];
  pushes: number;
  /** Pushes between macrotasks: the `.db` slices as the event loop saw them. */
  dbRuns: number[];
  dbCreated: number;
  freed: number;
  finished: number;
  afterStep?: (n: number) => void;
  afterPush?: (n: number) => void;
}

function newTrace(): Trace {
  return {
    csvCreated: 0,
    steps: [],
    stepYielded: [],
    pushes: 0,
    dbRuns: [],
    dbCreated: 0,
    freed: 0,
    finished: 0,
  };
}

interface CsvRecordingLike {
  step(maxLines: number): boolean;
  bytesRead(): number;
  finish(): RecordingRows;
  free(): void;
}

/** `w` with its recording readers wrapped to report into `trace`. */
function spyModule(trace: Trace): PowerzWasm {
  const real = w as unknown as {
    CsvRecording: new (bytes: Uint8Array) => CsvRecordingLike;
  };
  class SpyCsv {
    private inner: CsvRecordingLike;
    constructor(bytes: Uint8Array) {
      this.inner = new real.CsvRecording(bytes);
      trace.csvCreated++;
    }
    step(maxLines: number): boolean {
      trace.stepYielded.push(trace.steps.length > 0 && probeFired);
      trace.steps.push(maxLines);
      const more = this.inner.step(maxLines);
      armProbe();
      trace.afterStep?.(trace.steps.length);
      return more;
    }
    bytesRead(): number {
      return this.inner.bytesRead();
    }
    finish(): RecordingRows {
      trace.finished++;
      return this.inner.finish();
    }
    free(): void {
      trace.freed++;
      this.inner.free();
    }
  }
  let run = 0;
  class SpyDb {
    private inner = new w.DbRecording();
    constructor() {
      trace.dbCreated++;
      run = 0;
      probeArmed = false;
    }
    push(elapsed: string | undefined, numbers: Float64Array): boolean {
      if (!probeArmed) armProbe();
      else if (probeFired) {
        trace.dbRuns.push(run);
        run = 0;
        armProbe();
      }
      run++;
      trace.pushes++;
      const due = this.inner.push(elapsed, numbers);
      trace.afterPush?.(trace.pushes);
      return due;
    }
    lines(): number {
      return this.inner.lines();
    }
    finish(): RecordingRows {
      trace.finished++;
      trace.dbRuns.push(run);
      return this.inner.finish();
    }
    free(): void {
      trace.freed++;
      this.inner.free();
    }
  }
  return {
    ...(w as unknown as Record<string, unknown>),
    CsvRecording: SpyCsv,
    DbRecording: SpyDb,
  } as unknown as PowerzWasm;
}

// ---- files ----

function fixture(name: string): Uint8Array {
  return new Uint8Array(readFileSync(fileURLToPath(new URL(name, DATA))));
}

function picked(name: string, bytes: Uint8Array): PickedRecordingFile {
  return { name, size: bytes.length, arrayBuffer: async () => bytes.slice().buffer };
}

const CSV_HEADER = "ElapsedTime,Unix,VBUS,IBUS,DP,DM,CC1,CC2,TEMP,CHARGE,ENERGY,PWR";
const pad = (n: number, width: number) => String(n).padStart(width, "0");
function elapsed(ms: number): string {
  const h = Math.floor(ms / 3_600_000);
  const m = Math.floor(ms / 60_000) % 60;
  const s = Math.floor(ms / 1000) % 60;
  return `${h}:${pad(m, 2)}:${pad(s, 2)}.${pad(ms % 1000, 3)}`;
}

/** A recorder CSV of `lines` data lines, a damaged one and blank ones among them. */
function bigCsv(lines: number): Uint8Array {
  const out = [CSV_HEADER];
  for (let i = 0; i < lines; i++) {
    if (i % 997 === 500) out.push("damaged,line");
    else if (i % 1500 === 700) out.push("");
    else {
      const v = 5 + (i % 10) / 100;
      const a = 1 + (i % 7) / 10;
      out.push(
        `${elapsed(i * 100)},${1_700_000_000 + i},${v},${a},0.6,0.01,1.68,0,25,` +
          `${(i / 1e6).toFixed(6)},${(i / 2e5).toFixed(6)},${(v * a).toFixed(4)}`,
      );
    }
  }
  return new TextEncoder().encode(out.join("\n") + "\n");
}

const DB_TABLE = "powerz_01_02_03_04_05";

/** A vendor-shaped `.db` of `rows` data rows, every 1000th one damaged. */
function bigDb(rows: number): Uint8Array {
  const db = new SQL.Database();
  db.run(
    `CREATE TABLE "${DB_TABLE}" (ElapsedTime text, Unix real primary key, VBUS real, ` +
      "IBUS real, DP real, DM real, CC1 real, CC2 real, TEMP real, CHARGE real, ENERGY real)",
  );
  db.run(
    `CREATE TABLE "${DB_TABLE}_param" (name text, Unix text, StartTime text, SamplingRate text)`,
  );
  db.run("BEGIN");
  const stmt = db.prepare(`INSERT INTO "${DB_TABLE}" VALUES (?,?,?,?,?,?,?,?,?,?,?)`);
  for (let i = 0; i < rows; i++) {
    const damaged = i % 1000 === 999;
    stmt.run([
      damaged ? null : elapsed(i * 100),
      1_700_000_000 + i,
      damaged ? null : 12 + (i % 5) / 100,
      1 + (i % 3) / 10,
      0.6,
      0.01,
      1.68,
      0,
      25,
      i / 1e6,
      i / 1e5,
    ]);
  }
  stmt.free();
  db.run("COMMIT");
  const bytes = db.export();
  db.close();
  return bytes;
}

/** The rows of `bytes` run through one unsliced wasm `DbRecording`, as before. */
function dbReference(bytes: Uint8Array): RecordingRows {
  const db = new SQL.Database(bytes);
  try {
    const stmt = db.prepare(w.dbSelectRowsSql(DB_TABLE));
    const reader = new w.DbRecording();
    const numbers = new Float64Array(10);
    while (stmt.step()) {
      const row = stmt.get();
      for (let i = 0; i < 10; i++) {
        const v = row[i + 1];
        numbers[i] = typeof v === "number" ? v : NaN;
      }
      const e = row[0];
      reader.push(typeof e === "string" ? e : undefined, numbers);
    }
    stmt.free();
    return reader.finish();
  } finally {
    db.close();
  }
}

/** Recording-file commands over `module`, picking `files` in turn. */
function filesPicking(module: PowerzWasm, files: PickedRecordingFile[]): RecordingFiles {
  let picks = 0;
  return new RecordingFiles(
    async () => files[picks++] ?? null,
    wasmRecordingReader(async () => module, loadSqlJs),
  );
}

async function rejection(p: Promise<unknown>): Promise<UiError> {
  return p.then(
    () => {
      throw new Error("the call resolved");
    },
    (e: unknown) => e as UiError,
  );
}

const CSV_LINES = 12_001;
const DB_ROWS = 12_000;

describe("the read in slices (AC-3B70S6.1)", () => {
  it("a slice is 5,000 lines", () => {
    expect(RECORDING_SLICE_LINES).toBe(5000);
  });

  it("a CSV is stepped at most 5,000 lines at a time, yielding between steps", async () => {
    const trace = newTrace();
    const files = filesPicking(spyModule(trace), [picked("big.csv", bigCsv(CSV_LINES))]);
    const summary = await files.open(() => {});
    expect(summary).not.toBeNull();
    expect(trace.csvCreated).toBe(1);
    expect(trace.steps.length).toBeGreaterThanOrEqual(Math.ceil(CSV_LINES / 5000));
    for (const n of trace.steps) {
      expect(n).toBeGreaterThan(0);
      expect(n).toBeLessThanOrEqual(5000);
    }
    // Every step after the first comes after the event loop had a turn.
    expect(trace.stepYielded.slice(1)).toEqual(trace.steps.slice(1).map(() => true));
    expect(trace.finished).toBe(1);
  });

  it("a .db is read at most 5,000 rows at a time, yielding between slices", async () => {
    const trace = newTrace();
    const files = filesPicking(spyModule(trace), [picked("big.db", bigDb(DB_ROWS))]);
    const summary = await files.open(() => {});
    expect(summary).not.toBeNull();
    expect(trace.pushes).toBe(DB_ROWS);
    expect(trace.dbRuns.reduce((a, b) => a + b, 0)).toBe(DB_ROWS);
    expect(trace.dbRuns.length).toBeGreaterThanOrEqual(Math.ceil(DB_ROWS / 5000));
    for (const run of trace.dbRuns) expect(run).toBeLessThanOrEqual(5000);
  });
});

describe("Cancel during the read (AC-3B70S6.2)", () => {
  it("a CSV read stops before the next slice and frees its reader", async () => {
    const trace = newTrace();
    const files = filesPicking(spyModule(trace), [picked("big.csv", bigCsv(CSV_LINES))]);
    trace.afterStep = (n) => {
      if (n === 1) void files.cancel();
    };
    expect(await files.open(() => {})).toBeNull();
    expect(trace.steps).toHaveLength(1);
    expect(trace.freed).toBe(1);
    expect(trace.finished).toBe(0);
    expect((await rejection(files.rangeStats(0, 1000))).message).toBe("No recording file is open.");
  });

  it("a .db read stops before the next slice and frees its reader", async () => {
    const trace = newTrace();
    const files = filesPicking(spyModule(trace), [picked("big.db", bigDb(DB_ROWS))]);
    trace.afterPush = (n) => {
      if (n === 1) void files.cancel();
    };
    expect(await files.open(() => {})).toBeNull();
    expect(trace.pushes).toBeGreaterThan(0);
    expect(trace.pushes).toBeLessThanOrEqual(5000);
    expect(trace.freed).toBe(1);
    expect(trace.finished).toBe(0);
  });

  it("a read whose signal is already aborted reads no slice and frees what it made", async () => {
    for (const [name, bytes] of [
      ["big.csv", bigCsv(100)],
      ["big.db", bigDb(100)],
    ] as const) {
      const trace = newTrace();
      const read = wasmRecordingReader(async () => spyModule(trace), loadSqlJs);
      const controller = new AbortController();
      controller.abort();
      await expect(
        read(picked(name, bytes), () => {}, controller.signal),
        name,
      ).rejects.toThrow();
      expect(trace.steps, name).toHaveLength(0);
      expect(trace.pushes, name).toBe(0);
      expect(trace.finished, name).toBe(0);
      expect(trace.freed, name).toBe(trace.csvCreated + trace.dbCreated);
    }
  });
});

describe("the file open before a cancelled one (AC-3B70S6.3)", () => {
  for (const [name, bytes] of [
    ["big.csv", () => bigCsv(CSV_LINES)],
    ["big.db", () => bigDb(DB_ROWS)],
  ] as const) {
    it(`stays the one range, stats and close answer from (${name})`, async () => {
      const trace = newTrace();
      const files = filesPicking(spyModule(trace), [
        picked("recording-ours.csv", fixture("recording-ours.csv")),
        picked(name, bytes()),
      ]);
      expect((await files.open(() => {}))?.rows).toBe(5);
      const before = trace.steps.length + trace.pushes;
      trace.afterStep = (n) => {
        if (n === before + 1) void files.cancel();
      };
      trace.afterPush = (n) => {
        if (n === 1) void files.cancel();
      };
      expect(await files.open(() => {})).toBeNull();
      expect(trace.freed).toBeGreaterThanOrEqual(1);
      expect((await files.rangeStats(0, 2000))?.samples).toBe(5);
      const [vbus] = await files.range(["vbus"], 0, 2000, 512);
      expect(vbus?.buckets.map((b) => b.t_ms)).toEqual([0, 500, 1000, 1500, 2000]);
      expect(
        (await files.runProgress({ kind: "charge", step_pct: 25, ed_points: 0 }))?.samples,
      ).toBe(5);
      await files.close();
      expect((await rejection(files.rangeStats(0, 2000))).message).toBe(
        "No recording file is open.",
      );
    });
  }
});

describe("an open that is not cancelled (AC-3B70S6.4)", () => {
  function expectMonotone(progress: OpenProgress[]): void {
    for (let i = 1; i < progress.length; i++) {
      expect(progress[i]?.done ?? 0).toBeGreaterThanOrEqual(progress[i - 1]?.done ?? 0);
    }
  }

  it("a large CSV gives the one-shot reader's rows, counts, statistics and progress", async () => {
    const bytes = bigCsv(CSV_LINES);
    const want: FileSummary = openedFromRows("big.csv", w.readRecordingCsv(bytes)).summary;
    expect(want.skipped).toBeGreaterThan(0);
    const trace = newTrace();
    const files = filesPicking(spyModule(trace), [picked("big.csv", bytes)]);
    const progress: OpenProgress[] = [];
    const got = await files.open((p) => progress.push(p));
    expect(got).toEqual(want);
    expect(progress[0]).toEqual({ done: 0, total: 0 });
    expect(progress.at(-1)).toEqual({ done: bytes.length, total: bytes.length });
    expectMonotone(progress);
    expect(trace.steps.length).toBeGreaterThan(1);
  });

  it("a large .db gives the unsliced reader's rows, counts, statistics and progress", async () => {
    const bytes = bigDb(DB_ROWS);
    const want: FileSummary = openedFromRows("big.db", dbReference(bytes)).summary;
    expect(want.skipped).toBeGreaterThan(0);
    const trace = newTrace();
    const files = filesPicking(spyModule(trace), [picked("big.db", bytes)]);
    const progress: OpenProgress[] = [];
    const got = await files.open((p) => progress.push(p));
    expect(got).toEqual(want);
    expect(progress[0]).toEqual({ done: 0, total: 0 });
    expect(progress[1]).toEqual({ done: 0, total: DB_ROWS });
    expect(progress.at(-1)).toEqual({ done: DB_ROWS, total: DB_ROWS });
    expectMonotone(progress);
    expect(trace.dbRuns.length).toBeGreaterThan(1);
  });

  it("the committed fixtures, ours and the vendor's, open with their desktop counts", async () => {
    const cases: [string, [number, number, number]][] = [
      ["recording-ours.csv", [5, 1, 1]],
      ["recording-vendor.csv", [4, 2, 0]],
      ["recording-vendor.db", [5, 5, 0]],
    ];
    for (const [name, counts] of cases) {
      const trace = newTrace();
      const files = filesPicking(spyModule(trace), [picked(name, fixture(name))]);
      const s = await files.open(() => {});
      expect(s && [s.rows, s.skipped, s.left_out], name).toEqual(counts);
      // Read through the stepped readers, not a one-shot call.
      expect(trace.steps.length + trace.pushes, name).toBeGreaterThan(0);
      expect(trace.finished, name).toBe(1);
    }
  });
});
