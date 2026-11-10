/**
 * FR 3B70S7 AC-3B70S7.1 (regression): in the web session, samples that arrive before
 * the first valid temperature reading carry no TEMP in the live batch, the record CSV
 * and both exports; samples after it carry it as before.
 *
 * A `WebSession` driven by hand on Vitest's fake clock: the test calls `onTemp` and
 * `onBatch` as the driver would.
 */

import { fileURLToPath } from "node:url";
import initSqlJs, { type SqlJsStatic, type SqlValue } from "sql.js";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import type { BackendEvent } from "../backend";
import { CSV_HEADER } from "./csv";
import type { MeterDriver, MeterSample, PowerzWasm } from "./driver";
import { WebSession } from "./session";
import { loadWasm } from "./testWasm.test";
import { downloadSink, type Download } from "./webBackendFakes.test";

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

const sample = (ms: number): MeterSample => ({
  time_ms: ms,
  vbus_v: 5,
  ibus_a: 1,
  cc1_v: 1.65,
  cc2_v: 0,
  dp_v: 0.6,
  dm_v: 0.6,
});

const at = (ms: number): void => {
  vi.setSystemTime(T0 + ms);
};

/** A hand-driven session that keeps the live batches' `temp_c`, one per row. */
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

const TEMP = CSV_HEADER.split(",").indexOf("TEMP");

async function csvTemps(d: Download | undefined): Promise<(number | null)[]> {
  if (!d) throw new Error("nothing was downloaded");
  const lines = (await d.blob.text()).split("\n").filter((l) => l.length > 0);
  expect(lines[0]).toBe(CSV_HEADER);
  return lines
    .slice(1)
    .map((l) => l.split(",")[TEMP])
    .map((t) => (t === "" || t === undefined ? null : Number(t)));
}

async function dbTemps(d: Download | undefined): Promise<SqlValue[]> {
  if (!d) throw new Error("nothing was downloaded");
  const db = new SQL.Database(new Uint8Array(await d.blob.arrayBuffer()));
  try {
    const tables = db.exec("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name");
    const table = (tables[0]?.values ?? [])
      .map((r) => String(r[0]))
      .find((n) => !n.endsWith("_param"));
    if (!table) throw new Error("no data table");
    const q = `"${table.replaceAll('"', '""')}"`;
    return (db.exec(`SELECT TEMP FROM ${q} ORDER BY Unix`)[0]?.values ?? []).map(
      (r) => r[0] ?? null,
    );
  } finally {
    db.close();
  }
}

/**
 * The first 0.4 s: two batches before any reading, one after a snapshot that read
 * no temperature (`onTemp(null)`), then the first valid reading and two batches.
 */
function playFirstSecond(session: WebSession): void {
  at(0);
  session.onBatch([sample(0), sample(20), sample(40)]);
  at(100);
  session.onBatch([sample(60), sample(80), sample(100)]);
  at(200);
  session.onTemp(null);
  session.onBatch([sample(120), sample(140), sample(160), sample(180), sample(200)]);
  at(400);
  session.onTemp(33.5);
  session.onBatch([sample(220), sample(240)]);
  at(500);
  session.onBatch([sample(260), sample(280)]);
}

const EXPECTED: (number | null)[] = [
  ...Array.from({ length: 11 }, () => null),
  33.5,
  33.5,
  33.5,
  33.5,
];

describe("rows before the first valid temperature reading have no TEMP (AC-3B70S7.1)", () => {
  it("3b70s7: the live batches carry no temp_c before the first valid reading, then carry it", () => {
    const { session, live } = liveSession();
    playFirstSecond(session);
    expect(live).toEqual(EXPECTED);
  });

  it("3b70s7: the record CSV leaves TEMP empty before the first valid reading, then fills it", async () => {
    const { session, downloads } = liveSession();
    at(0);
    await session.startRecording("rec.csv");
    playFirstSecond(session);
    await session.stopRecording();
    const rec = downloads.find((d) => d.name === "rec.csv") ?? downloads[downloads.length - 1];
    expect(await csvTemps(rec)).toEqual(EXPECTED);
  });

  it("3b70s7: Export CSV and Export .db leave TEMP empty / NULL before the first valid reading", async () => {
    const { session, downloads, live } = liveSession();
    playFirstSecond(session);
    await session.exportSession("export.csv", () => {});
    await session.exportSessionDb("export.db", () => {});
    expect(await csvTemps(downloads[0])).toEqual(EXPECTED);
    expect(await dbTemps(downloads[1])).toEqual(EXPECTED);
    expect(live).toEqual(EXPECTED);
  });
});
