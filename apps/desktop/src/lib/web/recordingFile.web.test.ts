/**
 * FR AHTZW3: Open recording in the web edition, through the web backend with no meter.
 *
 * - AC-AHTZW3.1: `openRecordingFile` opens the browser's file picker for CSV and .db
 *   files (`browserPickRecordingFile`, `RECORDING_ACCEPT` in `./recordingFile`); a
 *   dismissed picker resolves `null` and changes nothing (an open file stays open).
 * - AC-AHTZW3.2: a CSV recording, ours or the vendor's, opens with the desktop's row,
 *   skipped and left-out counts and statistics: the committed fixtures
 *   (`src-tauri/tests/data/recording-*.csv`), whose desktop numbers
 *   `src-tauri/tests/recording_file_fixtures.rs` asserts.
 * - AC-AHTZW3.3: a vendor .db recording (`recording-vendor.db`, read with sql.js) keeps
 *   and skips the desktop's rows.
 * - AC-AHTZW3.4: `recordingFileRange`, `recordingFileRangeStats` and
 *   `recordingFileRunProgress` answer from the opened file's readings over its time
 *   range, widened to the rows beside a zoom as the desktop's recording view is.
 * - AC-AHTZW3.5: a file that is no recording is refused with the desktop's message (a
 *   usage error) and keeps the file open before; Cancel ends a running open as `null`.
 *
 * API pinned here: `createWebBackend({ pickRecordingFile })`, where
 * `PickRecordingFile = () => Promise<PickedRecordingFile | null>` and a picked file is
 * `{ name, size?, arrayBuffer() }` (a browser `File` is one).
 */

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import initSqlJs, { type SqlJsStatic } from "sql.js";
import { beforeAll, describe, expect, it } from "vitest";
import type { FileSummary, MinMaxAvg, OpenProgress, UiError } from "../backend";
import type { PowerzWasm } from "./driver";
import { RECORDING_ACCEPT, browserPickRecordingFile } from "./recordingFile";
import { loadWasm } from "./testWasm.test";
import { createWebBackend } from "./webBackend";

type Options = Parameters<typeof createWebBackend>[0];
type Backend = Awaited<ReturnType<typeof createWebBackend>>;

interface Picked {
  name: string;
  size?: number;
  arrayBuffer(): Promise<ArrayBuffer>;
}

const SQL_DIST = new URL("../../../node_modules/sql.js/dist/", import.meta.url);
const DATA = new URL("../../../src-tauri/tests/data/", import.meta.url);

let w: PowerzWasm;
let SQL: SqlJsStatic;
const loadSqlJs = (): Promise<SqlJsStatic> =>
  initSqlJs({ locateFile: (f: string) => fileURLToPath(new URL(f, SQL_DIST)) });

beforeAll(async () => {
  w = await loadWasm();
  SQL = await loadSqlJs();
});

function fixture(name: string): Uint8Array {
  return new Uint8Array(readFileSync(fileURLToPath(new URL(name, DATA))));
}

function picked(name: string, bytes: Uint8Array): Picked {
  return { name, size: bytes.length, arrayBuffer: async () => bytes.slice().buffer };
}

/** A backend with no meter whose picker hands out `files` in turn (`null`: dismissed). */
async function backendPicking(files: (Picked | null)[]): Promise<{
  backend: Backend;
  picks: () => number;
}> {
  let picks = 0;
  const backend = await createWebBackend({
    wasm: w,
    usb: null,
    hid: null,
    serial: null,
    pickDirectory: null,
    crossOriginIsolated: false,
    pageEvents: null,
    sqlJs: loadSqlJs,
    download: () => {},
    pickRecordingFile: async () => files[picks++] ?? null,
  } as unknown as Options);
  return { backend, picks: () => picks };
}

async function open(backend: Backend, progress: OpenProgress[] = []): Promise<FileSummary | null> {
  return backend.openRecordingFile((p: OpenProgress) => progress.push(p));
}

async function rejection(p: Promise<unknown>): Promise<UiError> {
  return p.then(
    () => {
      throw new Error("the call resolved");
    },
    (e: unknown) => e as UiError,
  );
}

function near(a: number, b: number, what: string): void {
  expect(Math.abs(a - b), `${what}: ${a} against ${b}`).toBeLessThanOrEqual(1e-9);
}

function mma(a: MinMaxAvg | null, [min, max, avg]: [number, number, number], what: string) {
  expect(a, `${what}: no value`).not.toBeNull();
  near(a?.min ?? NaN, min, `${what} min`);
  near(a?.max ?? NaN, max, `${what} max`);
  near(a?.avg ?? NaN, avg, `${what} avg`);
}

/** A sql.js database's bytes. */
function dbBytes(build: (run: (sql: string) => void) => void): Uint8Array {
  const db = new SQL.Database();
  build((sql) => db.run(sql));
  const bytes = db.export();
  db.close();
  return bytes;
}

const CSV_HEADER = "ElapsedTime,Unix,VBUS,IBUS,DP,DM,CC1,CC2,TEMP,CHARGE,ENERGY,PWR";
const csv = (text: string) => new TextEncoder().encode(text);

describe("the picker (AC-AHTZW3.1)", () => {
  it("offers CSV and .db files", () => {
    expect(RECORDING_ACCEPT).toBe(".csv,.db");
  });

  type Listener = () => void;
  class FakeInput {
    type = "";
    accept = "";
    files: Picked[] | null = null;
    clicks = 0;
    private listeners: Record<string, Listener[]> = {};
    addEventListener(type: string, fn: Listener): void {
      (this.listeners[type] ??= []).push(fn);
    }
    removeEventListener(type: string, fn: Listener): void {
      this.listeners[type] = (this.listeners[type] ?? []).filter((l) => l !== fn);
    }
    click(): void {
      this.clicks++;
    }
    fire(type: string): void {
      for (const l of [...(this.listeners[type] ?? [])]) l();
    }
  }
  function fakeDocument() {
    const inputs: FakeInput[] = [];
    const doc = {
      createElement(tag: string) {
        expect(tag).toBe("input");
        const input = new FakeInput();
        inputs.push(input);
        return input;
      },
    };
    return { doc: doc as unknown as Document, inputs };
  }

  it("opens a file input for CSV and .db files inside the click, resolving to the chosen file", async () => {
    const { doc, inputs } = fakeDocument();
    const chosen = browserPickRecordingFile(doc)();
    expect(inputs).toHaveLength(1);
    const input = inputs[0] as FakeInput;
    expect(input.type).toBe("file");
    expect(input.accept).toBe(RECORDING_ACCEPT);
    expect(input.clicks).toBe(1);
    input.files = [picked("run.csv", csv(`${CSV_HEADER}\n`))];
    input.fire("change");
    const file = await chosen;
    expect(file?.name).toBe("run.csv");
    expect(new TextDecoder().decode(await file?.arrayBuffer())).toBe(`${CSV_HEADER}\n`);
  });

  it("resolves null when the picker is dismissed", async () => {
    const { doc, inputs } = fakeDocument();
    const chosen = browserPickRecordingFile(doc)();
    (inputs[0] as FakeInput).fire("cancel");
    expect(await chosen).toBeNull();
  });
});

describe("Open recording in the web backend (AC-AHTZW3.1)", () => {
  it("asks the picker, needs no meter, and a dismissed picker resolves null", async () => {
    const { backend, picks } = await backendPicking([null]);
    expect(await open(backend)).toBeNull();
    expect(picks()).toBe(1);
    const error = await rejection(backend.recordingFileRange(["vbus"], 0, 1000, 10));
    expect(error.message).toBe("No recording file is open.");
  });

  it("a dismissed picker leaves the file already open as it was", async () => {
    const { backend } = await backendPicking([
      picked("recording-ours.csv", fixture("recording-ours.csv")),
      null,
    ]);
    const first = await open(backend);
    expect(first?.rows).toBe(5);
    expect(await open(backend)).toBeNull();
    const [vbus] = await backend.recordingFileRange(["vbus"], 0, 2000, 100);
    expect(vbus?.buckets.length).toBeGreaterThan(0);
  });
});

describe("a CSV recording (AC-AHTZW3.2)", () => {
  it("ours opens with the desktop's counts and statistics", async () => {
    const { backend } = await backendPicking([
      picked("recording-ours.csv", fixture("recording-ours.csv")),
    ]);
    const progress: OpenProgress[] = [];
    const s = await open(backend, progress);
    expect(s).not.toBeNull();
    if (!s) return;
    expect(s.name).toBe("recording-ours.csv");
    expect([s.rows, s.skipped, s.left_out]).toEqual([5, 1, 1]);
    expect(s.stats.samples).toBe(5);
    expect(s.stats.elapsed_ms).toBe(2000);
    near(s.stats.charge_ah, 0.0004, "charge (the file's own last CHARGE)");
    near(s.stats.energy_wh, 0.002, "energy (the file's own last ENERGY)");
    mma(s.stats.vbus, [4.9, 5.2, 5.04], "vbus");
    mma(s.stats.ibus, [0.8, 1.2, 1.0], "ibus");
    mma(s.stats.power, [4.16, 6.12, 5.036], "power");
    // The first report comes before anything is read; the last says it is all read.
    expect(progress[0]).toEqual({ done: 0, total: 0 });
    const last = progress.at(-1);
    expect(last?.total).toBeGreaterThan(0);
    expect(last?.done).toBe(last?.total);
  });

  it("the vendor's (BOM, CRLF) opens with the desktop's counts and statistics", async () => {
    const { backend } = await backendPicking([
      picked("recording-vendor.csv", fixture("recording-vendor.csv")),
    ]);
    const s = await open(backend);
    expect(s).not.toBeNull();
    if (!s) return;
    expect([s.rows, s.skipped, s.left_out]).toEqual([4, 2, 0]);
    expect(s.stats.elapsed_ms).toBe(3000);
    near(s.stats.charge_ah, 0.0013, "charge");
    near(s.stats.energy_wh, 0.0115, "energy");
    mma(s.stats.vbus, [8.9, 9.1, 9.0], "vbus");
    mma(s.stats.ibus, [0.0, 2.5, 1.625], "ibus");
    mma(s.stats.power, [0.0, 22.5, 14.675], "power");
  });

  it("a file whose extension is not .db is read as a CSV", async () => {
    const { backend } = await backendPicking([picked("export.txt", fixture("recording-ours.csv"))]);
    expect((await open(backend))?.rows).toBe(5);
  });
});

describe("a vendor .db recording (AC-AHTZW3.3)", () => {
  it("keeps and skips the desktop's rows, in the table the desktop picks", async () => {
    const { backend } = await backendPicking([
      picked("recording-vendor.db", fixture("recording-vendor.db")),
    ]);
    const progress: OpenProgress[] = [];
    const s = await open(backend, progress);
    expect(s).not.toBeNull();
    if (!s) return;
    expect(s.name).toBe("recording-vendor.db");
    expect([s.rows, s.skipped, s.left_out]).toEqual([5, 5, 0]);
    expect(s.stats.samples).toBe(5);
    expect(s.stats.elapsed_ms).toBe(3000);
    near(s.stats.charge_ah, 0.0009, "charge");
    near(s.stats.energy_wh, 0.0105, "energy");
    mma(s.stats.vbus, [11.9, 12.1, 12.0], "vbus");
    mma(s.stats.ibus, [1.0, 2.0, 1.48], "ibus");
    mma(s.stats.power, [12.0, 23.8, 17.748], "power");
    expect(progress[0]).toEqual({ done: 0, total: 0 });
    // Progress counts the table's rows, up to its highest rowid.
    expect(progress.at(-1)).toEqual({ done: 10, total: 10 });
  });

  it("the extension is matched ignoring case", async () => {
    const { backend } = await backendPicking([picked("RUN.DB", fixture("recording-vendor.db"))]);
    expect((await open(backend))?.rows).toBe(5);
  });

  it("the web edition's own .db export opens", async () => {
    const { backend } = await backendPicking([picked("web-export.db", fixture("web-export.db"))]);
    const s = await open(backend);
    expect(s?.rows).toBeGreaterThan(0);
    expect(s?.skipped).toBe(0);
  });
});

describe("the opened file's chart (AC-AHTZW3.4)", () => {
  it("shows its readings over its time range", async () => {
    const { backend } = await backendPicking([
      picked("recording-ours.csv", fixture("recording-ours.csv")),
    ]);
    await open(backend);
    const [vbus, power] = await backend.recordingFileRange(["vbus", "power"], 0, 2000, 512);
    expect(vbus?.series).toBe("vbus");
    expect(power?.series).toBe("power");
    const times = vbus?.buckets.map((b) => b.t_ms) ?? [];
    expect(times).toEqual([0, 500, 1000, 1500, 2000]);
    const at = (t: number) => vbus?.buckets.find((b) => b.t_ms === t);
    near(at(500)?.min ?? NaN, 5.1, "vbus at 500 ms");
    near(at(1500)?.max ?? NaN, 4.9, "vbus at 1500 ms");
    const p = power?.buckets.find((b) => b.t_ms === 500);
    near(p?.max ?? NaN, 6.12, "power at 500 ms");
  });

  it("a zoom between two rows still draws the rows beside it, as on the desktop", async () => {
    const { backend } = await backendPicking([
      picked("recording-ours.csv", fixture("recording-ours.csv")),
    ]);
    await open(backend);
    const [vbus] = await backend.recordingFileRange(["vbus"], 600, 900, 64);
    const times = vbus?.buckets.map((b) => b.t_ms) ?? [];
    expect(times).toContain(500);
    expect(times).toContain(1000);
  });

  it("the .db file's chart is at its rows' times", async () => {
    const { backend } = await backendPicking([
      picked("recording-vendor.db", fixture("recording-vendor.db")),
    ]);
    await open(backend);
    const [vbus] = await backend.recordingFileRange(["vbus"], 0, 3000, 512);
    // The Unix-timed row lands at 750 ms; the damaged rows draw nothing.
    expect(vbus?.buckets.map((b) => b.t_ms)).toEqual([0, 250, 750, 1500, 3000]);
  });

  it("the ruler's stats and run progress answer from the file's rows", async () => {
    const { backend } = await backendPicking([
      picked("recording-ours.csv", fixture("recording-ours.csv")),
    ]);
    await open(backend);
    const range = await backend.recordingFileRangeStats(0, 2000);
    expect(range?.samples).toBe(5);
    near(range?.vbus_avg_v ?? NaN, 5.04, "ruler vbus avg");
    near(range?.ibus_avg_a ?? NaN, 1.0, "ruler ibus avg");
    expect(await backend.recordingFileRangeStats(5000, 6000)).toBeNull();
    const run = await backend.recordingFileRunProgress({
      kind: "charge",
      step_pct: 25,
      ed_points: 0,
    });
    expect(run?.samples).toBe(5);
    expect(run?.run_t0_ms).toBe(0);
    expect(run?.run_t1_ms).toBe(2000);
  });

  it("closing the file leaves no file open", async () => {
    const { backend } = await backendPicking([
      picked("recording-ours.csv", fixture("recording-ours.csv")),
    ]);
    await open(backend);
    await backend.closeRecordingFile();
    const error = await rejection(backend.recordingFileRange(["vbus"], 0, 2000, 10));
    expect(error.kind).toBe("usage");
    expect(error.message).toBe("No recording file is open.");
    expect((await rejection(backend.recordingFileRangeStats(0, 2000))).message).toBe(
      "No recording file is open.",
    );
  });
});

describe("refusals and Cancel (AC-AHTZW3.5)", () => {
  const noRecording = (): [string, Uint8Array][] => [
    ["empty.csv", new Uint8Array()],
    ["notes.csv", csv("time,volts\n0,5\n")],
    ["header-only.csv", csv(`${CSV_HEADER}\n`)],
    ["picture.csv", new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x00, 0x0a, 0x1a, 0x0a])],
    ["garbage.db", csv("this is no database at all, just some text\n")],
    [
      "other.db",
      dbBytes((run) => {
        run("CREATE TABLE notes (a text, b real)");
        run("CREATE TABLE notes_param (name text)");
        run("INSERT INTO notes VALUES ('x', 1)");
      }),
    ],
    [
      "empty-table.db",
      dbBytes((run) => {
        run(
          'CREATE TABLE "powerz_01_02_03_04_05" (ElapsedTime text, Unix real primary key, ' +
            "VBUS real, IBUS real, DP real, DM real, CC1 real, CC2 real, TEMP real, " +
            "CHARGE real, ENERGY real)",
        );
        run(
          'CREATE TABLE "powerz_01_02_03_04_05_param" (name text, Unix text, StartTime text, SamplingRate text)',
        );
      }),
    ],
  ];
  /** The desktop's words for each file above (`recording_file.rs`, `reader.rs`). */
  const MESSAGES: Record<string, string> = {
    "empty.csv": "This file is empty.",
    "notes.csv": "This file's header is not a recorder header.",
    "header-only.csv": "This file holds no readable row.",
    "picture.csv": "This file is not a text file, so it is not a recording.",
    "garbage.db": "This file is not a database that can be opened.",
    "other.db": "This database is not a recording.",
    "empty-table.db": "This file holds no readable row.",
  };

  it("a file that is no recording is refused with the desktop's message", async () => {
    for (const [name, bytes] of noRecording()) {
      const { backend } = await backendPicking([picked(name, bytes)]);
      const error = await rejection(open(backend));
      expect(error.kind, name).toBe("usage");
      expect(error.message, name).toBe(MESSAGES[name]);
    }
  });

  it("a refused file keeps the file open before it", async () => {
    const { backend } = await backendPicking([
      picked("recording-ours.csv", fixture("recording-ours.csv")),
      picked("notes.csv", csv("time,volts\n0,5\n")),
    ]);
    await open(backend);
    await rejection(open(backend));
    const range = await backend.recordingFileRangeStats(0, 2000);
    expect(range?.samples).toBe(5);
  });

  it("Cancel ends a running open as null and opens nothing", async () => {
    let release: (bytes: ArrayBuffer) => void = () => {};
    const slow: Picked = {
      name: "recording-ours.csv",
      arrayBuffer: () => new Promise<ArrayBuffer>((resolve) => (release = resolve)),
    };
    const { backend } = await backendPicking([slow]);
    const progress: OpenProgress[] = [];
    const opening = open(backend, progress);
    // Let the open reach the read.
    for (let i = 0; i < 20 && progress.length === 0; i++) await Promise.resolve();
    await new Promise((r) => setTimeout(r, 0));
    await backend.cancelRecordingFile();
    release(fixture("recording-ours.csv").slice().buffer);
    expect(await opening).toBeNull();
    const error = await rejection(backend.recordingFileRange(["vbus"], 0, 2000, 10));
    expect(error.message).toBe("No recording file is open.");
  });

  it("Cancel keeps the file open before the cancelled one", async () => {
    let release: (bytes: ArrayBuffer) => void = () => {};
    const slow: Picked = {
      name: "recording-vendor.csv",
      arrayBuffer: () => new Promise<ArrayBuffer>((resolve) => (release = resolve)),
    };
    const { backend } = await backendPicking([
      picked("recording-ours.csv", fixture("recording-ours.csv")),
      slow,
    ]);
    await open(backend);
    const opening = open(backend);
    await new Promise((r) => setTimeout(r, 0));
    await backend.cancelRecordingFile();
    release(fixture("recording-vendor.csv").slice().buffer);
    expect(await opening).toBeNull();
    expect((await backend.recordingFileRangeStats(0, 2000))?.samples).toBe(5);
  });

  it("an open after a Cancel opens again", async () => {
    let release: (bytes: ArrayBuffer) => void = () => {};
    const slow: Picked = {
      name: "a.csv",
      arrayBuffer: () => new Promise<ArrayBuffer>((resolve) => (release = resolve)),
    };
    const { backend } = await backendPicking([
      slow,
      picked("recording-vendor.csv", fixture("recording-vendor.csv")),
    ]);
    const opening = open(backend);
    await new Promise((r) => setTimeout(r, 0));
    await backend.cancelRecordingFile();
    release(fixture("recording-ours.csv").slice().buffer);
    expect(await opening).toBeNull();
    expect((await open(backend))?.rows).toBe(4);
  });
});
