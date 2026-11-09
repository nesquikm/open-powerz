/**
 * FR AHTZW3: Open recording in the web edition. The browser's file picker offers CSV
 * and `.db` files (AC-AHTZW3.1); the chosen file opens with no meter, and the recording
 * view's range and close commands answer from it.
 */

import type {
  BucketsFor,
  FileSummary,
  HistorySeries,
  OpenProgress,
  RangeStats,
  RunProgress,
  RunProgressQuery,
} from "../backend";
import type { SqlJsStatic } from "sql.js";
import { HistoryColumns, SessionStats } from "../history";
import type { PowerzWasm, RecordingRows } from "./driver";
import { usage, type LoadSqlJs } from "./session";

/** The file types the picker offers: the desktop dialog's CSV and vendor `.db` files. */
export const RECORDING_ACCEPT = ".csv,.db";

/** A file the user chose (a browser `File` is one). */
export interface PickedRecordingFile {
  name: string;
  /** Bytes, when the browser reports it. */
  size?: number;
  arrayBuffer(): Promise<ArrayBuffer>;
}

/** Opens the browser's file picker; `null` when it was dismissed. */
export type PickRecordingFile = () => Promise<PickedRecordingFile | null>;

interface FileInputLike {
  type: string;
  accept: string;
  files: ArrayLike<PickedRecordingFile> | null;
  click(): void;
  addEventListener(type: string, fn: () => void): void;
  removeEventListener(type: string, fn: () => void): void;
}

/** The picker over `doc`: an `<input type="file">` clicked synchronously, inside the
 * user's click, resolving to the chosen file or `null` when dismissed. */
export function browserPickRecordingFile(doc?: Document): PickRecordingFile {
  return () => {
    const input = (doc ?? document).createElement("input") as unknown as FileInputLike;
    input.type = "file";
    input.accept = RECORDING_ACCEPT;
    const chosen = new Promise<PickedRecordingFile | null>((resolve) => {
      const done = () => {
        input.removeEventListener("change", onChange);
        input.removeEventListener("cancel", onCancel);
      };
      const onChange = () => {
        done();
        resolve(input.files?.[0] ?? null);
      };
      const onCancel = () => {
        done();
        resolve(null);
      };
      input.addEventListener("change", onChange);
      input.addEventListener("cancel", onCancel);
    });
    input.click();
    return chosen;
  };
}

/** The refusal of a range or close-side command with no file open. */
const NO_FILE = "No recording file is open.";

/** An opened recording file's readings, as the recording view asks for them. */
export interface OpenedRecording {
  range<const S extends readonly HistorySeries[]>(
    series: S,
    t0Ms: number,
    t1Ms: number,
    buckets: number,
  ): BucketsFor<S>;
  rangeStats(t0Ms: number, t1Ms: number): RangeStats | null;
  runProgress(query: RunProgressQuery): RunProgress | null;
}

/** Reads a picked file into an opened recording and its summary. */
export type ReadRecordingFile = (
  file: PickedRecordingFile,
  onProgress: (progress: OpenProgress) => void,
) => Promise<{ summary: FileSummary; opened: OpenedRecording }>;

/** The web edition's recording-file commands: one file open at a time. */
export class RecordingFiles {
  private opened: OpenedRecording | null = null;
  /** Bumped by Cancel: an open that started under an older one ends as `null`. */
  private generation = 0;

  constructor(
    private readonly pick: PickRecordingFile,
    private readonly read: ReadRecordingFile,
  ) {}

  /** Picks a file and opens it; `null` when the picker was dismissed, which leaves the
   * file already open as it was. A Cancel during the open ends it as `null` and also
   * leaves the file already open as it was. */
  async open(onProgress: (progress: OpenProgress) => void): Promise<FileSummary | null> {
    const generation = this.generation;
    const file = await this.pick();
    if (!file || generation !== this.generation) return null;
    let result: Awaited<ReturnType<ReadRecordingFile>>;
    try {
      result = await this.read(file, onProgress);
    } catch (e) {
      if (generation !== this.generation) return null;
      throw e;
    }
    if (generation !== this.generation) return null;
    this.opened = result.opened;
    return result.summary;
  }

  /** Ends a running open as `null`; the file already open stays open. */
  async cancel(): Promise<void> {
    this.generation++;
  }

  async close(): Promise<void> {
    this.opened = null;
  }

  private current(): OpenedRecording {
    if (!this.opened) throw usage(NO_FILE);
    return this.opened;
  }

  async range<const S extends readonly HistorySeries[]>(
    series: S,
    t0Ms: number,
    t1Ms: number,
    buckets: number,
  ): Promise<BucketsFor<S>> {
    return this.current().range(series, t0Ms, t1Ms, buckets);
  }

  async rangeStats(t0Ms: number, t1Ms: number): Promise<RangeStats | null> {
    return this.current().rangeStats(t0Ms, t1Ms);
  }

  async runProgress(query: RunProgressQuery): Promise<RunProgress | null> {
    return this.current().runProgress(query);
  }
}

/** The file's name, without a folder. */
function baseName(name: string): string {
  return name.split(/[\\/]/).pop() ?? name;
}

/** `rows` as an opened recording and its summary, as the desktop's `FileBuilder` makes
 * them: the history and stats of the kept rows, the file's own last CHARGE and ENERGY
 * winning over a sum of the rows. */
export function openedFromRows(
  name: string,
  rows: RecordingRows,
): { summary: FileSummary; opened: OpenedRecording } {
  const history = new HistoryColumns(Math.max(1, rows.samples.length), "f64");
  const stats = new SessionStats();
  for (const m of rows.samples) {
    const s = {
      t_ms: m.time_ms,
      vbus_v: m.vbus_v,
      ibus_a: m.ibus_a,
      cc1_v: m.cc1_v,
      cc2_v: m.cc2_v,
      dp_v: m.dp_v,
      dm_v: m.dm_v,
    };
    history.push(s);
    stats.push(s);
  }
  const snapshot = stats.snapshot();
  snapshot.charge_ah = rows.charge_ah ?? snapshot.charge_ah;
  snapshot.energy_wh = rows.energy_wh ?? snapshot.energy_wh;
  const opened: OpenedRecording = {
    range: (series, t0Ms, t1Ms, buckets) =>
      history.buckets(series, t0Ms, t1Ms, buckets) as BucketsFor<typeof series>,
    rangeStats: (t0Ms, t1Ms) => history.rangeStats(t0Ms, t1Ms),
    runProgress: (query) => history.runProgress(query),
  };
  return {
    summary: {
      name: baseName(name),
      rows: rows.rows,
      skipped: rows.skipped,
      left_out: rows.left_out,
      stats: snapshot,
    },
    opened,
  };
}

/** A failed call's message, or `fallback`. */
function messageOf(e: unknown, fallback: string): string {
  const { message } = (e ?? {}) as { message?: unknown };
  return typeof message === "string" ? message : fallback;
}

/** The desktop's refusals of a `.db` file (`powerz_record::db`). */
const NOT_A_DATABASE = "This file is not a database that can be opened.";
const NOT_A_RECORDING = "This database is not a recording.";
const UNREADABLE_DB = "The database could not be read.";

/** The rows of the vendor `.db` `bytes`, read with sql.js by the desktop's SQL and
 * rules (`powerz_record::db`): the same table picked, the same rows kept and skipped.
 * Progress counts the table's rows, up to its highest rowid. */
export function readRecordingDb(
  w: PowerzWasm,
  SQL: SqlJsStatic,
  bytes: Uint8Array,
  onProgress: (progress: OpenProgress) => void,
): RecordingRows {
  let db: InstanceType<SqlJsStatic["Database"]>;
  try {
    db = new SQL.Database(bytes);
  } catch {
    throw usage(NOT_A_DATABASE);
  }
  try {
    let tables: [string, boolean][];
    try {
      tables = (db.exec(w.dbTableNamesSql())[0]?.values ?? []).map(
        ([name, plain]) => [String(name), plain !== 0 && plain !== null] as [string, boolean],
      );
    } catch {
      throw usage(NOT_A_DATABASE);
    }
    const columnsOf = (table: string): string[] =>
      (db.exec(w.dbTableColumnsSql(), [table])[0]?.values ?? []).map(([name]) => String(name));
    const table = w.dbPickDataTable(tables, columnsOf);
    if (table === undefined || table === null) throw usage(NOT_A_RECORDING);

    let total = 0;
    try {
      const top = db.exec(w.dbRowTotalSql(table))[0]?.values[0]?.[0];
      total = typeof top === "number" && top > 0 ? Math.floor(top) : 0;
    } catch {
      total = 0;
    }
    const doneOf = (lines: number) => (total === 0 ? lines : Math.min(lines, total));
    onProgress({ done: 0, total });

    const reader = new w.DbRecording();
    let stmt: ReturnType<typeof db.prepare> | null = null;
    try {
      try {
        stmt = db.prepare(w.dbSelectRowsSql(table));
      } catch {
        throw usage(UNREADABLE_DB);
      }
      const numbers = new Float64Array(10);
      for (;;) {
        let row: ReturnType<typeof stmt.get>;
        try {
          if (!stmt.step()) break;
          row = stmt.get();
        } catch {
          throw usage(UNREADABLE_DB);
        }
        const elapsed = row[0];
        for (let i = 0; i < 10; i++) {
          const v = row[i + 1];
          numbers[i] = typeof v === "number" ? v : NaN;
        }
        if (reader.push(typeof elapsed === "string" ? elapsed : undefined, numbers)) {
          onProgress({ done: doneOf(reader.lines()), total });
        }
      }
      onProgress({ done: doneOf(Math.max(total, reader.lines())), total });
    } catch (e) {
      reader.free();
      throw e;
    } finally {
      stmt?.free();
    }
    return reader.finish();
  } finally {
    db.close();
  }
}

/** Reads a picked file with the wasm module's recording readers: a `.db` (the name
 * ends in `.db`, any case) with the desktop's `.db` rules over sql.js, anything else
 * with the desktop's CSV reader, whose progress counts the file's bytes. */
export function wasmRecordingReader(
  module: () => Promise<PowerzWasm>,
  sqlJs?: LoadSqlJs,
): ReadRecordingFile {
  return async (file, onProgress) => {
    onProgress({ done: 0, total: 0 });
    const w = await module();
    const bytes = new Uint8Array(await file.arrayBuffer());
    if (/\.db$/i.test(file.name)) {
      if (!sqlJs) throw usage("The .db reader is not available in this browser.");
      const SQL = await sqlJs();
      let rows: RecordingRows;
      try {
        rows = readRecordingDb(w, SQL, bytes, onProgress);
      } catch (e) {
        throw usage(messageOf(e, UNREADABLE_DB));
      }
      return openedFromRows(file.name, rows);
    }
    let rows: RecordingRows;
    try {
      rows = w.readRecordingCsv(bytes);
    } catch (e) {
      throw usage(messageOf(e, "This file is not a recording."));
    }
    onProgress({ done: bytes.length, total: bytes.length });
    return openedFromRows(file.name, rows);
  };
}
