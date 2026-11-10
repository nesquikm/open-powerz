/**
 * FR JFRPME: the web session's history keeps the meter's temperature, so both session
 * exports carry it in TEMP, and Open recording reads it back.
 *
 * - AC-JFRPME.1: Export CSV writes each sample's temperature in TEMP: the latest
 *   snapshot reading not older than the age limit (2 s); rows without a current
 *   reading keep TEMP empty.
 * - AC-JFRPME.2: Export .db writes the same temperatures, NULL where the CSV is empty.
 * - AC-JFRPME.3: one temperature entry per change, not per sample or snapshot; Clear
 *   history drops them with the samples.
 * - AC-JFRPME.4: a web export opened again with Open recording's readers (the wasm CSV
 *   reader, the sql.js `.db` reader) reads the same TEMP values back.
 *
 * A `WebSession` driven by hand on Vitest's fake clock, and the demo meter end to end.
 */

import { fileURLToPath } from "node:url";
import initSqlJs, { type Database, type SqlJsStatic, type SqlValue } from "sql.js";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import type { BackendEvent, Batch } from "../backend";
import { CSV_HEADER, sig8 } from "./csv";
import type { MeterDriver, MeterSample, PowerzWasm, RecordingRows } from "./driver";
import { readRecordingDb } from "./recordingFile";
import { WebSession } from "./session";
import { loadWasm } from "./testWasm.test";
import { createWebBackend } from "./webBackend";
import { downloadSink, settle, type Download } from "./webBackendFakes.test";

type Options = Parameters<typeof createWebBackend>[0];

const SQL_DIST = new URL("../../../node_modules/sql.js/dist/", import.meta.url);
const T0 = 1_790_000_000_000;

let w: PowerzWasm;
let SQL: SqlJsStatic;
const loadSqlJs = (): Promise<SqlJsStatic> =>
  initSqlJs({ locateFile: (f: string) => fileURLToPath(new URL(f, SQL_DIST)) });

beforeAll(async () => {
  SQL = await loadSqlJs();
});

beforeEach(async () => {
  w = await loadWasm();
  vi.useFakeTimers({
    toFake: ["setTimeout", "clearTimeout", "setInterval", "clearInterval", "Date"],
  });
  vi.setSystemTime(T0);
});

afterEach(() => {
  vi.useRealTimers();
});

/** A sample at meter time `ms`, about 5 V / 1 A. */
const sample = (ms: number): MeterSample => ({
  time_ms: ms,
  vbus_v: 5 + ms / 1e6,
  ibus_a: 1,
  cc1_v: 1.65,
  cc2_v: 0,
  dp_v: 0.6,
  dm_v: 0.6,
});

/** A session driven by hand: no driver runs, the test calls `onTemp` and `onBatch`. */
function handSession(): { session: WebSession; downloads: Download[] } {
  const sink = downloadSink();
  const session = new WebSession({
    deviceId: "test",
    sessionId: 1,
    onEvent: () => {},
    onEnd: () => {},
    wasm: w,
    download: sink.download,
    sqlJs: loadSqlJs,
  });
  session.driver = { mode: "queue", rateCode: 2, hid: false } as unknown as MeterDriver;
  return { session, downloads: sink.downloads };
}

/** Sets the fake clock to `T0 + ms`. */
const at = (ms: number): void => {
  vi.setSystemTime(T0 + ms);
};

/**
 * The timeline (session time = meter time = ms since T0):
 * - 0: one sample, no reading yet;
 * - 100: 31.5 °C read; samples 100, 200; 300: samples 300, 400;
 * - 500: 31.5 °C read again (the same entry); sample 500;
 * - 600: 32.25 °C read; samples 600, 700;
 * - 3000: no reading since 600 (expired at 2600); samples 3000, 3100;
 * - 3200: 32.25 °C read again, after the old one expired; sample 3200.
 */
function playTimeline(session: WebSession): void {
  at(0);
  session.onBatch([sample(0)]);
  at(100);
  session.onTemp(31.5);
  session.onBatch([sample(100), sample(200)]);
  at(300);
  session.onBatch([sample(300), sample(400)]);
  at(500);
  session.onTemp(31.5);
  session.onBatch([sample(500)]);
  at(600);
  session.onTemp(32.25);
  session.onBatch([sample(600), sample(700)]);
  at(3_000);
  session.onBatch([sample(3_000), sample(3_100)]);
  at(3_200);
  session.onTemp(32.25);
  session.onBatch([sample(3_200)]);
}

/** The temperature of each timeline row. */
const TIMELINE_TEMPS: (number | null)[] = [
  null,
  31.5,
  31.5,
  31.5,
  31.5,
  31.5,
  32.25,
  32.25,
  null,
  null,
  32.25,
];

/** A downloaded CSV's data rows, split into fields. */
async function csvRows(d: Download | undefined): Promise<string[][]> {
  if (!d) throw new Error("nothing was downloaded");
  const lines = (await d.blob.text()).split("\n").filter((l) => l.length > 0);
  expect(lines[0]).toBe(CSV_HEADER);
  return lines.slice(1).map((l) => l.split(","));
}

const TEMP = CSV_HEADER.split(",").indexOf("TEMP");

/** The TEMP column of CSV rows: a number, or `null` when empty. */
const csvTemps = (rows: string[][]): (number | null)[] =>
  rows.map((r) => (r[TEMP] === "" || r[TEMP] === undefined ? null : Number(r[TEMP])));

async function openDb(d: Download | undefined): Promise<Database> {
  if (!d) throw new Error("nothing was downloaded");
  return new SQL.Database(new Uint8Array(await d.blob.arrayBuffer()));
}

const all = (db: Database, sql: string): SqlValue[][] => db.exec(sql)[0]?.values ?? [];
const quoted = (name: string): string => `"${name.replaceAll('"', '""')}"`;

/** The TEMP column of a downloaded `.db`, in row order. */
async function dbTemps(d: Download | undefined): Promise<SqlValue[]> {
  const db = await openDb(d);
  try {
    const table = all(db, "SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
      .map((r) => String(r[0]))
      .find((n) => !n.endsWith("_param"));
    if (!table) throw new Error("no data table");
    return all(db, `SELECT TEMP FROM ${quoted(table)} ORDER BY Unix`).map((r) => r[0] ?? null);
  } finally {
    db.close();
  }
}

/** The temperatures a reader's rows carry (`null` where it read none). */
const rowTemps = (rows: RecordingRows): (number | null)[] =>
  rows.samples.map((s) => {
    const t = (s as MeterSample & { temp_c?: number | null }).temp_c;
    return typeof t === "number" ? t : null;
  });

describe("Export CSV writes TEMP (AC-JFRPME.1)", () => {
  it("each row's TEMP is the latest current reading, empty with none or an expired one", async () => {
    const { session, downloads } = handSession();
    playTimeline(session);
    const result = await session.exportSession("export.csv", () => {});
    expect(result?.rows).toBe(TIMELINE_TEMPS.length);
    const rows = await csvRows(downloads[0]);
    expect(rows.map((r) => r[TEMP])).toEqual([
      "",
      "31.5",
      "31.5",
      "31.5",
      "31.5",
      "31.5",
      "32.25",
      "32.25",
      "",
      "",
      "32.25",
    ]);
  });

  it("a reading older than the age limit at a sample leaves its TEMP empty", async () => {
    const { session, downloads } = handSession();
    at(0);
    session.onTemp(30);
    session.onBatch([sample(0), sample(1_000), sample(2_000)]);
    // The reading expires at session time 2000; at 2500 it is 2.5 s old.
    at(2_500);
    session.onBatch([sample(2_500)]);
    await session.exportSession("export.csv", () => {});
    expect(csvTemps(await csvRows(downloads[0]))).toEqual([30, 30, 30, null]);
  });
});

describe("Export .db writes TEMP (AC-JFRPME.2)", () => {
  it("the same temperatures as the CSV, NULL where the CSV is empty", async () => {
    const { session, downloads } = handSession();
    playTimeline(session);
    await session.exportSession("export.csv", () => {});
    const result = await session.exportSessionDb("export.db", () => {});
    expect(result?.rows).toBe(TIMELINE_TEMPS.length);
    const csv = csvTemps(await csvRows(downloads[0]));
    const db = await dbTemps(downloads[1]);
    expect(db).toEqual(TIMELINE_TEMPS);
    expect(db).toEqual(csv);
  });
});

describe("one entry per change; Clear history drops them (AC-JFRPME.3)", () => {
  it("the history keeps one entry per temperature change, not per sample or snapshot", () => {
    const { session } = handSession();
    playTimeline(session);
    // 31.5 (read twice), 32.25, and 32.25 again after it had expired.
    expect(session.temps.entries).toHaveLength(3);
  });

  it("many samples and repeated snapshots of one temperature stay one entry", () => {
    const { session } = handSession();
    for (let k = 0; k < 20; k++) {
      at(k * 100);
      if (k % 5 === 0) session.onTemp(30);
      session.onBatch([sample(k * 100), sample(k * 100 + 50)]);
    }
    expect(session.history.length).toBe(40);
    expect(session.temps.entries).toHaveLength(1);
  });

  it("Clear history drops the entries with the samples; later rows get their own", async () => {
    const { session, downloads } = handSession();
    playTimeline(session);
    await session.commands().clearHistory();
    expect(session.history.length).toBe(0);
    expect(session.temps.entries).toHaveLength(0);

    // The 32.25 °C read at 3200 is still current; a new reading follows.
    at(3_300);
    session.onBatch([sample(3_300)]);
    at(3_400);
    session.onTemp(29.5);
    session.onBatch([sample(3_400), sample(3_500)]);
    expect(session.temps.entries).toHaveLength(2);

    await session.exportSession("export.csv", () => {});
    await session.exportSessionDb("export.db", () => {});
    expect(csvTemps(await csvRows(downloads[0]))).toEqual([32.25, 29.5, 29.5]);
    expect(await dbTemps(downloads[1])).toEqual([32.25, 29.5, 29.5]);
  });
});

describe("Open recording reads the exported TEMP back (AC-JFRPME.4)", () => {
  it("the CSV reader reads each row's temperature", async () => {
    const { session, downloads } = handSession();
    playTimeline(session);
    await session.exportSession("export.csv", () => {});
    const bytes = new Uint8Array(await (downloads[0] as Download).blob.arrayBuffer());
    const rows = w.readRecordingCsv(bytes);
    expect(rows.rows).toBe(TIMELINE_TEMPS.length);
    expect(rowTemps(rows)).toEqual(TIMELINE_TEMPS);
  });

  it("the .db reader reads each row's temperature", async () => {
    const { session, downloads } = handSession();
    playTimeline(session);
    await session.exportSessionDb("export.db", () => {});
    const bytes = new Uint8Array(await (downloads[0] as Download).blob.arrayBuffer());
    const rows = await readRecordingDb(w, SQL, bytes, () => {});
    expect(rows.rows).toBe(TIMELINE_TEMPS.length);
    expect(rowTemps(rows)).toEqual(TIMELINE_TEMPS);
  });
});

describe("the demo meter end to end (AC-JFRPME.1, AC-JFRPME.2, AC-JFRPME.4)", () => {
  it("both exports carry the temperatures the live batches showed", async () => {
    const sink = downloadSink();
    const backend = await settle(
      createWebBackend({
        wasm: w,
        usb: null,
        download: sink.download,
        sqlJs: loadSqlJs,
      } as unknown as Options),
    );
    const demo = (await settle(backend.listDevices())).find((d) => d.demo_scenario !== null);
    if (!demo) throw new Error("no demo meter listed");
    const events: BackendEvent[] = [];
    await settle(backend.connect(demo.id, 50, (e) => events.push(e)));
    await vi.advanceTimersByTimeAsync(3_000);
    // Exports need no timers: the batches so far are exactly the history.
    const batches = events.flatMap((e): Batch[] => (e.type === "batch" ? [e.data] : []));
    const live = batches.flatMap((b) => b.samples.map(() => b.temp_c ?? null));
    expect(live.some((t) => typeof t === "number")).toBe(true);

    await backend.exportSession("export.csv", () => {});
    await backend.exportSessionDb("export.db", () => {});
    const csv = csvTemps(await csvRows(sink.downloads[0]));
    expect(
      csv.some((t) => t !== null),
      "the CSV's TEMP is never empty throughout",
    ).toBe(true);
    const csvText = (await csvRows(sink.downloads[0])).map((r) => r[TEMP]);
    expect(csvText).toEqual(live.map((t) => (t === null ? "" : sig8(t))));
    expect(await dbTemps(sink.downloads[1])).toEqual(live);

    const csvBytes = new Uint8Array(await (sink.downloads[0] as Download).blob.arrayBuffer());
    expect(rowTemps(w.readRecordingCsv(csvBytes))).toEqual(csv);
    await settle(backend.disconnect());
  });
});

/**
 * FR Z99166 AC-Z99166.2 (regression): a batch whose session times run more than the
 * snapshot age (2 s) past the temperature reading carries that reading on every row of
 * both exports, as the live batch judges it on arrival; a batch that arrives with no
 * current reading keeps TEMP empty.
 */
describe("a backlog batch past the snapshot age keeps its TEMP (AC-Z99166.2)", () => {
  /** A hand-driven session that also keeps the live batches' `temp_c`, one per row. */
  function liveSession(): { session: WebSession; downloads: Download[]; live: (number | null)[] } {
    const sink = downloadSink();
    const live: (number | null)[] = [];
    const session = new WebSession({
      deviceId: "test",
      sessionId: 1,
      onEvent: (e: BackendEvent) => {
        if (e.type === "batch") live.push(...e.data.samples.map(() => e.data.temp_c ?? null));
      },
      onEnd: () => {},
      wasm: w,
      download: sink.download,
      sqlJs: loadSqlJs,
    });
    session.driver = { mode: "queue", rateCode: 2, hid: false } as unknown as MeterDriver;
    return { session, downloads: sink.downloads, live };
  }

  const span = (from: number, to: number, step: number): MeterSample[] => {
    const out: MeterSample[] = [];
    for (let ms = from; ms <= to; ms += step) out.push(sample(ms));
    return out;
  };

  it("z99166: a 3 s backlog read in one batch carries the reading on every row", async () => {
    const { session, downloads, live } = liveSession();
    at(0);
    session.onTemp(30);
    // Read after a pause: 3 s of samples at once, session times 0..3000.
    session.onBatch(span(0, 3_000, 100));
    expect(session.history.timeAt(session.history.length - 1)).toBe(3_000);
    const expected = Array.from({ length: 31 }, () => 30);
    expect(live).toEqual(expected);

    await session.exportSession("export.csv", () => {});
    await session.exportSessionDb("export.db", () => {});
    expect(csvTemps(await csvRows(downloads[0]))).toEqual(expected);
    expect(await dbTemps(downloads[1])).toEqual(expected);
  });

  it("z99166: after a re-based session clock the backlog carries the reading on every row", async () => {
    const { session, downloads, live } = liveSession();
    at(0);
    session.onTemp(29);
    session.onBatch([sample(0)]);
    // A reconnect or a logo write: the meter clock jumps, the session re-bases at the
    // wall clock (500), and the batch runs 3 s on from there (session 500..3500).
    at(500);
    session.onTemp(31);
    session.onBatch(span(100_000, 103_000, 250));
    expect(session.history.timeAt(session.history.length - 1)).toBe(3_500);
    const expected = [29, ...Array.from({ length: 13 }, () => 31)];
    expect(live).toEqual(expected);

    await session.exportSession("export.csv", () => {});
    await session.exportSessionDb("export.db", () => {});
    const csv = csvTemps(await csvRows(downloads[0]));
    expect(csv).toEqual(expected);
    expect(csv).toEqual(live);
    expect(await dbTemps(downloads[1])).toEqual(expected);
  });

  it("z99166: a batch that arrives with no current reading keeps TEMP empty", async () => {
    const { session, downloads, live } = liveSession();
    at(500);
    session.onTemp(31);
    session.onBatch(span(100_000, 103_000, 250));
    // 5.5 s after the reading, nothing newer: the next samples have no temperature.
    at(6_000);
    session.onBatch([sample(103_250), sample(103_500)]);
    expect(live.slice(-2)).toEqual([null, null]);

    await session.exportSession("export.csv", () => {});
    await session.exportSessionDb("export.db", () => {});
    const expected = [...Array.from({ length: 13 }, () => 31), null, null];
    expect(live).toEqual(expected);
    expect(csvTemps(await csvRows(downloads[0]))).toEqual(expected);
    expect(await dbTemps(downloads[1])).toEqual(expected);
  });

  it("z99166: a stale reading never applies to a backlog that arrives after it expired", async () => {
    const { session, downloads, live } = liveSession();
    at(0);
    session.onTemp(30);
    session.onBatch([sample(0)]);
    // 2.5 s later a 3 s backlog arrives; the 30 °C reading is no longer current.
    at(2_500);
    session.onBatch(span(100, 3_100, 500));
    const expected = [30, null, null, null, null, null, null, null];
    expect(live).toEqual(expected);
    await session.exportSession("export.csv", () => {});
    await session.exportSessionDb("export.db", () => {});
    expect(csvTemps(await csvRows(downloads[0]))).toEqual(expected);
    expect(await dbTemps(downloads[1])).toEqual(expected);
  });
});
