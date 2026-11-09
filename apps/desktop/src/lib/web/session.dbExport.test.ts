/**
 * FR AHTZW2: Export .db in the web edition, over the demo meter. The downloaded file
 * is read back with sql.js.
 *
 * - AC-AHTZW2.1: Export .db downloads a .db file of the session so far and reports
 *   its progress (it used to reject with "exportSessionDb is not available in the
 *   browser yet.").
 * - AC-AHTZW2.2: the desktop's layout: a data table named after the session's local
 *   start time with the desktop's columns, and its `_param` table.
 * - AC-AHTZW2.3: its rows match the desktop export of the same session: elapsed time,
 *   Unix time, the readings, charge and energy (checked against the CSV export of the
 *   same session, which matches the desktop recorder's rows).
 * - AC-AHTZW2.4 (web half): the committed browser-written fixture the desktop reader
 *   test opens (`src-tauri/tests/data/web-export.db`) has the layout this writer makes.
 *
 * Runs on Vitest's fake clock (setTimeout and Date).
 */

import { existsSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import initSqlJs, { type Database, type SqlJsStatic, type SqlValue } from "sql.js";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { CSV_HEADER } from "./csv";
import type { PowerzWasm } from "./driver";
import { loadWasm } from "./testWasm.test";
import { createWebBackend } from "./webBackend";
import { downloadSink, settle, type Download } from "./webBackendFakes.test";

type Options = Parameters<typeof createWebBackend>[0];
type Backend = Awaited<ReturnType<typeof createWebBackend>>;

const SQL_DIST = new URL("../../../node_modules/sql.js/dist/", import.meta.url);
const FIXTURE = new URL("../../../src-tauri/tests/data/web-export.db", import.meta.url);

const DATA_COLUMNS: [string, string][] = [
  ["ElapsedTime", "text"],
  ["Unix", "real"],
  ["VBUS", "real"],
  ["IBUS", "real"],
  ["DP", "real"],
  ["DM", "real"],
  ["CC1", "real"],
  ["CC2", "real"],
  ["TEMP", "real"],
  ["CHARGE", "real"],
  ["ENERGY", "real"],
];
const PARAM_COLUMNS: [string, string][] = [
  ["name", "text"],
  ["Unix", "text"],
  ["StartTime", "text"],
  ["SamplingRate", "text"],
];

let w: PowerzWasm;
let SQL: SqlJsStatic;
/** sql.js as the backend loads it in the browser, here from node_modules. */
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
  vi.setSystemTime(1_790_000_000_000);
});

afterEach(() => {
  vi.useRealTimers();
});

interface Run {
  backend: Backend;
  downloads: Download[];
}

/** Connects to the built-in demo meter at `rateSps`. */
async function connectDemo(rateSps = 50): Promise<Run> {
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
  await settle(backend.connect(demo.id, rateSps, () => {}));
  return { backend, downloads: sink.downloads };
}

/** Streams for `ms`, then exports the session as `.db`. */
async function exportDb(ms = 2_000, rateSps = 50) {
  const run = await connectDemo(rateSps);
  await vi.advanceTimersByTimeAsync(ms);
  const progress: { done: number; total: number }[] = [];
  const result = await settle(run.backend.exportSessionDb("export.db", (p) => progress.push(p)));
  return { run, result, progress };
}

async function openDownload(d: Download | undefined): Promise<Database> {
  if (!d) throw new Error("nothing was downloaded");
  return new SQL.Database(new Uint8Array(await d.blob.arrayBuffer()));
}

const all = (db: Database, sql: string): SqlValue[][] => db.exec(sql)[0]?.values ?? [];
const quoted = (name: string): string => `"${name.replaceAll('"', '""')}"`;
const tables = (db: Database): string[] =>
  all(db, "SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name").map((r) =>
    String(r[0]),
  );
const dataTable = (db: Database): string => {
  const t = tables(db).find((n) => !n.endsWith("_param"));
  if (!t) throw new Error("no data table");
  return t;
};

const pad = (n: number): string => String(n).padStart(2, "0");

describe("Export .db in the browser (AC-AHTZW2.1)", () => {
  it("downloads a SQLite file of the session so far and reports its progress", async () => {
    const { run, result, progress } = await exportDb();
    expect(result).toMatchObject({ path: "export.db" });
    expect(result?.rows).toBeGreaterThan(50);
    expect(progress.length).toBeGreaterThan(0);
    expect(progress.at(-1)).toEqual({ done: result?.rows, total: result?.rows });
    expect(progress.every((p) => p.done <= p.total)).toBe(true);

    expect(run.downloads.map((d) => d.name)).toEqual(["export.db"]);
    const bytes = new Uint8Array(await (run.downloads[0] as Download).blob.arrayBuffer());
    expect(new TextDecoder().decode(bytes.slice(0, 16))).toBe("SQLite format 3\0");

    const db = await openDownload(run.downloads[0]);
    const count = all(db, `SELECT count(*) FROM ${quoted(dataTable(db))}`)[0]?.[0];
    expect(count).toBe(result?.rows);
    db.close();
    await settle(run.backend.disconnect());
  });

  it("a second export downloads a second file with the rows streamed since", async () => {
    const { run, result } = await exportDb(1_000);
    await vi.advanceTimersByTimeAsync(1_000);
    const again = await settle(run.backend.exportSessionDb("again.db", () => {}));
    expect(run.downloads.map((d) => d.name)).toEqual(["export.db", "again.db"]);
    expect(again?.rows).toBeGreaterThan(result?.rows ?? Infinity);
    await settle(run.backend.disconnect());
  });
});

describe("the desktop's layout (AC-AHTZW2.2)", () => {
  it("has a powerz_MM_DD_HH_MM_SS data table with the desktop's columns and its param table", async () => {
    const { run } = await exportDb();
    const db = await openDownload(run.downloads[0]);
    const table = dataTable(db);
    expect(table).toMatch(/^powerz_\d\d_\d\d_\d\d_\d\d_\d\d$/);
    expect(tables(db)).toEqual([table, `${table}_param`]);

    const info = (t: string) => all(db, `PRAGMA table_info(${quoted(t)})`);
    expect(info(table).map((r) => [r[1], String(r[2]).toLowerCase()])).toEqual(DATA_COLUMNS);
    // `Unix` is the primary key.
    expect(
      info(table)
        .filter((r) => r[5] !== 0)
        .map((r) => r[1]),
    ).toEqual(["Unix"]);
    expect(info(`${table}_param`).map((r) => [r[1], String(r[2]).toLowerCase()])).toEqual(
      PARAM_COLUMNS,
    );
    db.close();
    await settle(run.backend.disconnect());
  });

  it("names the table after the session's local start time and fills one param row", async () => {
    const { run } = await exportDb(2_000, 50);
    const db = await openDownload(run.downloads[0]);
    const table = dataTable(db);
    const firstUnix = Number(
      all(db, `SELECT Unix FROM ${quoted(table)} ORDER BY Unix LIMIT 1`)[0]?.[0],
    );
    // The session started on the test's clock.
    expect(firstUnix).toBeGreaterThanOrEqual(1_790_000_000);
    expect(firstUnix).toBeLessThan(1_790_000_005);

    const start = new Date(Math.floor(firstUnix) * 1000);
    const local = `${start.getFullYear()}-${pad(start.getMonth() + 1)}-${pad(start.getDate())} ${pad(start.getHours())}:${pad(start.getMinutes())}:${pad(start.getSeconds())}`;
    const name = `powerz_${pad(start.getMonth() + 1)}_${pad(start.getDate())}_${pad(start.getHours())}_${pad(start.getMinutes())}_${pad(start.getSeconds())}`;
    expect(table).toBe(name);

    const params = all(db, `SELECT * FROM ${quoted(`${table}_param`)}`);
    expect(params).toHaveLength(1);
    const [pName, pUnix, pStart, pRate] = params[0] ?? [];
    expect(pName).toBe(name);
    expect(pUnix).toMatch(/^\d+\.\d{3}$/);
    expect(Number(pUnix)).toBeCloseTo(firstUnix, 2);
    expect(pStart).toBe(local);
    expect(pRate).toBe("50");
    db.close();
    await settle(run.backend.disconnect());
  });
});

describe("the desktop's rows (AC-AHTZW2.3)", () => {
  it("matches the session's CSV export row for row", async () => {
    const { run, result } = await exportDb(3_000);
    const db = await openDownload(run.downloads[0]);
    const table = dataTable(db);
    const rows = all(
      db,
      `SELECT ElapsedTime, Unix, VBUS, IBUS, DP, DM, CC1, CC2, TEMP, CHARGE, ENERGY FROM ${quoted(table)} ORDER BY Unix`,
    );
    expect(rows).toHaveLength(result?.rows ?? -1);
    db.close();

    // The CSV export of the same session: the history only grows, so the .db's rows are
    // its first rows.
    await settle(run.backend.exportSession("export.csv", () => {}));
    const lines = (await (run.downloads[1] as Download).blob.text())
      .split("\n")
      .filter((l) => l.length > 0);
    expect(lines[0]).toBe(CSV_HEADER);
    const csv = lines.slice(1).map((l) => l.split(","));
    expect(csv.length).toBeGreaterThanOrEqual(rows.length);

    expect(rows[0]?.[0]).toBe("0:00:00.000");
    let prevUnix = -Infinity;
    rows.forEach((r, i) => {
      const c = csv[i] ?? [];
      expect(r[0], `row ${i} ElapsedTime`).toBe(c[0]);
      const unix = Number(r[1]);
      expect(Math.abs(unix - Number(c[1])), `row ${i} Unix`).toBeLessThan(0.0015);
      expect(unix, `row ${i} Unix rises`).toBeGreaterThan(prevUnix);
      prevUnix = unix;
      // VBUS, IBUS, DP, DM, CC1, CC2: the CSV has eight significant digits.
      for (let k = 2; k <= 7; k++) {
        const want = Number(c[k]);
        expect(typeof r[k], `row ${i} column ${k}`).toBe("number");
        expect(Math.abs(Number(r[k]) - want), `row ${i} column ${k}`).toBeLessThanOrEqual(
          1e-9 + 1e-7 * Math.abs(want),
        );
      }
      expect(r[8] === null || typeof r[8] === "number", `row ${i} TEMP`).toBe(true);
      // CHARGE, ENERGY.
      for (const [k, ck] of [
        [9, 9],
        [10, 10],
      ] as const) {
        const want = Number(c[ck]);
        expect(Math.abs(Number(r[k]) - want), `row ${i} column ${k}`).toBeLessThanOrEqual(
          1e-12 + 1e-6 * Math.abs(want),
        );
      }
    });
    // The demo meter draws about 5 V / 1 A, so charge and energy grow.
    const last = rows.at(-1) ?? [];
    expect(Number(last[2])).toBeCloseTo(5, 0);
    expect(Number(last[9])).toBeGreaterThan(0);
    expect(Number(last[10])).toBeGreaterThan(Number(last[9]));
    await settle(run.backend.disconnect());
  });
});

describe("the browser-written fixture the desktop reader opens (AC-AHTZW2.4)", () => {
  it("is committed and has the layout this writer makes", async () => {
    const path = fileURLToPath(FIXTURE);
    expect(existsSync(path), `${path} is missing`).toBe(true);
    const fixture = new SQL.Database(new Uint8Array(readFileSync(path)));
    const { run } = await exportDb(1_000);
    const fresh = await openDownload(run.downloads[0]);

    // The statements SQLite keeps, with the table name taken out.
    const schema = (db: Database): string[] => {
      const t = dataTable(db);
      return all(db, "SELECT sql FROM sqlite_master WHERE type = 'table' ORDER BY name").map((r) =>
        String(r[0]).replaceAll(t, "T"),
      );
    };
    expect(schema(fixture)).toEqual(schema(fresh));
    const n = Number(all(fixture, `SELECT count(*) FROM ${quoted(dataTable(fixture))}`)[0]?.[0]);
    expect(n).toBeGreaterThan(0);
    fixture.close();
    fresh.close();
    await settle(run.backend.disconnect());
  });
});
