/**
 * FR EAKFEC: the PD analyzer's Open capture and Import vendor log in the web edition.
 * The browser's file picker offers `.pdcap` and `.sqlite` files (AC-EAKFEC.1); the
 * chosen file opens with no meter, by the desktop's rules: a pdcap file through the
 * wasm module's loader and capture, a vendor PD log through sql.js running the
 * desktop's SQL into the wasm module's import rules (AC-EAKFEC.2, .3). The opened
 * capture answers record details, its cable identity, Clear and release (AC-EAKFEC.5).
 */

import type { SqlJsStatic } from "sql.js";
import type { CableView, PdDetails, PdOpened, UiError } from "../backend";
import { usage, type LoadSqlJs } from "./session";

/** The file types each picker offers: the desktop dialogs' filters. */
export const PDCAP_ACCEPT = ".pdcap";
export const VENDOR_PD_ACCEPT = ".sqlite";

/** A file the user chose (a browser `File` is one). */
export interface PickedPdFile {
  name: string;
  arrayBuffer(): Promise<ArrayBuffer>;
}

/** Opens the browser's file picker; `null` when it was dismissed. */
export type PickPdFile = () => Promise<PickedPdFile | null>;

interface FileInputLike {
  type: string;
  accept: string;
  files: ArrayLike<PickedPdFile> | null;
  click(): void;
  addEventListener(type: string, fn: () => void): void;
  removeEventListener(type: string, fn: () => void): void;
}

/** The picker for `accept` over `doc`: an `<input type="file">` clicked synchronously,
 * inside the user's click, resolving to the chosen file or `null` when dismissed. A new
 * pick supersedes one still pending (a picker the browser never answered): the earlier
 * pick resolves as dismissed and its input's late answer changes nothing. */
export function browserPickPdFile(accept: string, doc?: Document): PickPdFile {
  let supersede: (() => void) | null = null;
  return () => {
    supersede?.();
    const input = (doc ?? document).createElement("input") as unknown as FileInputLike;
    input.type = "file";
    input.accept = accept;
    const chosen = new Promise<PickedPdFile | null>((resolve) => {
      const done = () => {
        input.removeEventListener("change", onChange);
        input.removeEventListener("cancel", onCancel);
        if (supersede === onCancel) supersede = null;
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
      supersede = onCancel;
    });
    input.click();
    return chosen;
  };
}

/** The wasm `OpenedPd`: a capture opened from a file. */
export interface OpenedPdLike {
  /** The rows, status samples and counts, once (`undefined` after). */
  take(): Omit<PdOpened, "path"> | undefined;
  details(index: number | bigint): PdDetails | undefined;
  latestCable(): CableView | null;
  free(): void;
}

/** The wasm `VendorPdLog`: a vendor PD log taken in one result row at a time. */
export interface VendorPdLogLike {
  pushRecord(timeS: number, vbusV: number, ibusA: number, raw: Uint8Array | undefined): boolean;
  pushChart(values: Float64Array): boolean;
  finish(): OpenedPdLike;
  free(): void;
}

/** The desktop import's statements and limits (`powerz_protocol::pd_capture`). */
export interface VendorPdSql {
  has_table: string;
  oversized: string;
  records: string;
  chart: string;
  max_raw_bytes: number;
  fetch: number;
}

/** The parts of the wasm module the PD file commands use. */
export interface PdFilesWasm {
  openPdcap(bytes: Uint8Array): OpenedPdLike;
  vendorPdSql(): VendorPdSql;
  vendorPdRefusal(detail: string | undefined): UiError;
  VendorPdLog: new (oversized: number) => VendorPdLogLike;
}

/** The desktop's refusal of a record the opened capture does not keep. */
const NOT_IN_OPENED = "That PD record is not in the opened capture.";

/** A wasm `BindError` (or anything thrown) as the usage error the UI shows. */
function refusal(e: unknown, fallback: string): UiError {
  const { message } = (e ?? {}) as { message?: unknown };
  return usage(typeof message === "string" ? message : fallback);
}

/** A number column, NaN for anything else (the wasm side reads NaN as no number). */
const num = (v: unknown): number => (typeof v === "number" ? v : NaN);

/** The vendor PD log `bytes` read with sql.js by the desktop's SQL and rules: the same
 * rows imported, skipped and left out, the same refusals. */
export function importVendorPdLog(
  w: PdFilesWasm,
  SQL: SqlJsStatic,
  bytes: Uint8Array,
): OpenedPdLike {
  // The desktop's words: why the database could not be read, or that it is no PD log.
  const refused = (detail: string | undefined): UiError =>
    refusal(w.vendorPdRefusal(detail), "Could not import the vendor PD log.");
  const failed = (e: unknown): UiError =>
    refused(
      typeof (e as { message?: unknown })?.message === "string"
        ? (e as { message: string }).message
        : String(e),
    );
  let db: InstanceType<SqlJsStatic["Database"]>;
  try {
    db = new SQL.Database(bytes);
    // As the desktop opens it: the file's schema calls no functions, nothing is written,
    // and a file that is no database fails here.
    db.exec("PRAGMA trusted_schema = OFF; PRAGMA query_only = ON;");
    db.exec("SELECT count(*) FROM sqlite_master");
  } catch (e) {
    throw failed(e);
  }
  const sql = w.vendorPdSql();
  try {
    const has = (table: string): boolean => db.exec(sql.has_table, [table]).length > 0;
    if (!has("pd_table")) throw refused(undefined);
    const oversized = num(db.exec(sql.oversized, [sql.max_raw_bytes])[0]?.values[0]?.[0]);
    const log = new w.VendorPdLog(Number.isNaN(oversized) ? 0 : oversized);
    try {
      const each = (query: string, params: number[], take: (row: unknown[]) => boolean) => {
        const stmt = db.prepare(query, params);
        try {
          while (stmt.step()) if (!take(stmt.get())) break;
        } finally {
          stmt.free();
        }
      };
      each(sql.records, [sql.max_raw_bytes, sql.fetch], (row) =>
        log.pushRecord(
          num(row[0]),
          num(row[1]),
          num(row[2]),
          row[3] instanceof Uint8Array ? row[3] : undefined,
        ),
      );
      if (has("pd_chart")) {
        const values = new Float64Array(5);
        each(sql.chart, [sql.fetch], (row) => {
          for (let i = 0; i < 5; i++) values[i] = num(row[i]);
          return log.pushChart(values);
        });
      }
    } catch (e) {
      log.free();
      throw e;
    }
    return log.finish();
  } catch (e) {
    if (typeof e === "object" && e !== null && "kind" in e) throw e;
    throw failed(e);
  } finally {
    db.close();
  }
}

/** The web edition's PD file commands: one opened capture at a time, kept in the page
 * for its details and cable identity until released. */
export class PdFiles {
  private opened: OpenedPdLike | null = null;

  constructor(
    private readonly pickCapture: PickPdFile,
    private readonly pickVendorLog: PickPdFile,
    private readonly module: () => Promise<PdFilesWasm>,
    private readonly sqlJs?: LoadSqlJs,
  ) {}

  /** Open capture: picks a pdcap file and opens it; `null` when the picker was
   * dismissed. A refused file keeps the capture opened before. */
  async open(): Promise<PdOpened | null> {
    const file = await this.pickCapture();
    if (!file) return null;
    const w = await this.module();
    const bytes = new Uint8Array(await file.arrayBuffer());
    let opened: OpenedPdLike;
    try {
      opened = w.openPdcap(bytes);
    } catch (e) {
      throw refusal(e, "Could not open the PD capture.");
    }
    return this.show(file.name, opened);
  }

  /** Import vendor log: picks a vendor PD log and imports it read-only; `null` when the
   * picker was dismissed. A refused file keeps the capture opened before. */
  async import(): Promise<PdOpened | null> {
    const file = await this.pickVendorLog();
    if (!file) return null;
    const w = await this.module();
    if (!this.sqlJs) {
      throw refusal(w.vendorPdRefusal("no SQLite reader in this browser"), "");
    }
    const SQL = await this.sqlJs();
    const bytes = new Uint8Array(await file.arrayBuffer());
    return this.show(file.name, importVendorPdLog(w, SQL, bytes));
  }

  /** Whether a capture opened from a file is held. */
  get held(): boolean {
    return this.opened !== null;
  }

  /** The details of record `index` of the opened capture. */
  details(index: number): PdDetails {
    const d = this.opened?.details(BigInt(index));
    if (!d) throw usage(NOT_IN_OPENED);
    return d;
  }

  /** The newest cable identity the opened capture holds; `null` for none. */
  cable(): CableView | null {
    return this.opened?.latestCable() ?? null;
  }

  /** Lets go of the opened capture; returns whether there was one. */
  release(): boolean {
    const had = this.opened !== null;
    this.opened?.free();
    this.opened = null;
    return had;
  }

  private show(name: string, opened: OpenedPdLike): PdOpened {
    const view = opened.take();
    if (!view) {
      opened.free();
      throw usage("Could not open the PD capture.");
    }
    this.release();
    this.opened = opened;
    return { path: name, ...view };
  }
}
