/**
 * FR EAKFEC: the PD analyzer's Open capture and Import vendor log in the web edition,
 * through the web backend over the real wasm module and sql.js.
 *
 * - AC-EAKFEC.1: `pdOpen` opens the browser's picker for `.pdcap` files and `pdImport`
 *   for `.sqlite` files (`browserPickPdFile`, `PDCAP_ACCEPT`, `VENDOR_PD_ACCEPT`); a
 *   dismissed picker resolves `null` and keeps the capture opened before.
 * - AC-EAKFEC.2, .3: the committed fixtures (`src-tauri/tests/data/pd-capture.pdcap`,
 *   `pd-vendor.sqlite`) open as `pd-open-expected.json` says: every row, status sample,
 *   count, record details and the cable identity. The desktop test
 *   `src-tauri/tests/pd_open_fixtures.rs` asserts the same file on the desktop's open.
 * - AC-EAKFEC.4: a file that is no pdcap file or no vendor PD log is refused with the
 *   desktop's message (a usage error) and the capture opened before stays.
 * - AC-EAKFEC.5: with no meter the opened capture answers details (`opened`), its cable
 *   identity, Clear (`{ next_index: 0, status_floor_ms: 0 }`) and release; the
 *   per-meter commands see the same opened capture.
 * - AC-EAKFEC.6: a capture saved with `pdSave` in the browser opens again with its rows.
 *
 * API pinned here: `createWebBackend({ pickPdCapture, pickVendorPdLog })`, each a
 * `() => Promise<{ name, arrayBuffer() } | null>` (a browser `File` is one).
 */

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import initSqlJs, { type SqlJsStatic } from "sql.js";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import type { BackendEvent, PdOpened, PdRow, UiError } from "../backend";
import type { PowerzWasm } from "./driver";
import { PDCAP_ACCEPT, VENDOR_PD_ACCEPT, browserPickPdFile } from "./pdFiles";
import { realMeterDevice } from "./realMeter.test";
import { loadWasm } from "./testWasm.test";
import { createWebBackend } from "./webBackend";
import { FakeUsbNavigator, downloadSink, settle, type Download } from "./webBackendFakes.test";

type Options = Parameters<typeof createWebBackend>[0];
type Backend = Awaited<ReturnType<typeof createWebBackend>>;

interface Picked {
  name: string;
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

afterEach(() => {
  vi.useRealTimers();
});

function fixture(name: string): Uint8Array {
  return new Uint8Array(readFileSync(fileURLToPath(new URL(name, DATA))));
}

interface Seen {
  opened: Omit<PdOpened, "path">;
  details: unknown[];
  cable: unknown;
}
const EXPECTED = JSON.parse(new TextDecoder().decode(fixture("pd-open-expected.json"))) as Record<
  "pdcap" | "vendor",
  Seen
>;

function picked(name: string, bytes: Uint8Array): Picked {
  return { name, arrayBuffer: async () => bytes.slice().buffer };
}

/** A backend with no meter whose pickers hand out `captures` and `logs` in turn
 * (`null`: dismissed). */
async function backendPicking(
  captures: (Picked | null)[],
  logs: (Picked | null)[] = [],
  extra: Partial<Record<string, unknown>> = {},
): Promise<Backend> {
  let c = 0;
  let l = 0;
  return createWebBackend({
    wasm: w,
    usb: null,
    hid: null,
    serial: null,
    pickDirectory: null,
    crossOriginIsolated: false,
    pageEvents: null,
    sqlJs: loadSqlJs,
    download: () => {},
    pickPdCapture: async () => captures[c++] ?? null,
    pickVendorPdLog: async () => logs[l++] ?? null,
    ...extra,
  } as unknown as Options);
}

async function rejection(p: Promise<unknown>): Promise<UiError> {
  return p.then(
    () => {
      throw new Error("the call resolved");
    },
    (e: unknown) => e as UiError,
  );
}

/** What the page sees of `opened`, as the desktop test writes it: the view without the
 * path, the details of every row (plain JSON) and the cable identity. */
async function seen(backend: Backend, opened: PdOpened | null): Promise<Seen> {
  if (!opened) throw new Error("nothing opened");
  const view: Partial<PdOpened> = { ...opened };
  delete view.path;
  const details = [];
  for (const r of opened.rows) details.push(await backend.pdDetails(r.index, true));
  const cable = await backend.pdCableSeen();
  return JSON.parse(JSON.stringify({ opened: view, details, cable })) as Seen;
}

/** A sql.js database's bytes. */
function dbBytes(sql: string): Uint8Array {
  const db = new SQL.Database();
  db.run(sql);
  const bytes = db.export();
  db.close();
  return bytes;
}

describe("the pickers (AC-EAKFEC.1)", () => {
  it("offer the desktop dialogs' file types", () => {
    expect(PDCAP_ACCEPT).toBe(".pdcap");
    expect(VENDOR_PD_ACCEPT).toBe(".sqlite");
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
      for (const fn of this.listeners[type] ?? []) fn();
    }
  }
  const docWith = (input: FakeInput) => ({ createElement: () => input }) as unknown as Document;

  it("a file input is clicked with the type's filter and answers the chosen file", async () => {
    const input = new FakeInput();
    const pick = browserPickPdFile(PDCAP_ACCEPT, docWith(input))();
    expect(input.type).toBe("file");
    expect(input.accept).toBe(".pdcap");
    expect(input.clicks).toBe(1);
    const file = picked("a.pdcap", new Uint8Array());
    input.files = [file];
    input.fire("change");
    expect(await pick).toBe(file);
  });

  it("a dismissed file input answers null", async () => {
    const input = new FakeInput();
    const pick = browserPickPdFile(VENDOR_PD_ACCEPT, docWith(input))();
    expect(input.accept).toBe(".sqlite");
    input.fire("cancel");
    expect(await pick).toBeNull();
  });

  it("a dismissed picker changes nothing: the capture opened before stays", async () => {
    const b = await backendPicking(
      [picked("pd-capture.pdcap", fixture("pd-capture.pdcap")), null],
      [null],
    );
    const first = await b.pdOpen();
    expect(first?.rows.length).toBe(EXPECTED.pdcap.opened.rows.length);
    expect(await b.pdOpen()).toBeNull();
    expect(await b.pdImport()).toBeNull();
    const row = first?.rows[1] as PdRow;
    expect(await b.pdDetails(row.index, true)).toEqual(EXPECTED.pdcap.details[1]);
  });

  /** A document handing out `inputs` in turn, one per picker call. */
  const docOf = (inputs: FakeInput[]) => {
    let k = 0;
    return {
      createElement: () => {
        const next = inputs[k++];
        if (!next) throw new Error("more file inputs than expected");
        return next;
      },
    } as unknown as Document;
  };

  /** Whether `p` has settled after the pending microtasks ran. */
  const settledNow = async (p: Promise<unknown>): Promise<boolean> => {
    let done = false;
    void p.then(
      () => (done = true),
      () => (done = true),
    );
    for (let i = 0; i < 5; i++) await Promise.resolve();
    return done;
  };

  // FR Z99166 AC-Z99166.3 (regression): a new click while an earlier picker never
  // answered (no change, no cancel) opens the picker again; the earlier pick resolves
  // as dismissed and changes nothing.
  it("z99166: a new Open capture pick while one is pending opens a new picker; the first answers null", async () => {
    const first = new FakeInput();
    const second = new FakeInput();
    const pick = browserPickPdFile(PDCAP_ACCEPT, docOf([first, second]));
    const p1 = pick();
    expect(first.clicks).toBe(1);
    expect(await settledNow(p1)).toBe(false);

    const p2 = pick();
    expect(second.clicks).toBe(1);
    expect(second.accept).toBe(".pdcap");
    expect(await settledNow(p1), "the first pick resolves once a new one starts").toBe(true);
    expect(await p1).toBeNull();

    const file = picked("b.pdcap", new Uint8Array());
    second.files = [file];
    second.fire("change");
    expect(await p2).toBe(file);
  });

  it("z99166: a new Import vendor log pick supersedes a pending one; a late answer to the first changes nothing", async () => {
    const first = new FakeInput();
    const second = new FakeInput();
    const pick = browserPickPdFile(VENDOR_PD_ACCEPT, docOf([first, second]));
    const p1 = pick();
    const p2 = pick();
    expect(second.clicks).toBe(1);
    expect(await settledNow(p1), "the first pick resolves once a new one starts").toBe(true);
    expect(await p1).toBeNull();

    // The first input answering late resolves nothing more and leaves the second pending.
    first.files = [picked("late.sqlite", new Uint8Array())];
    first.fire("change");
    expect(await settledNow(p2)).toBe(false);

    const file = picked("b.sqlite", new Uint8Array());
    second.files = [file];
    second.fire("change");
    expect(await p2).toBe(file);
  });

  it("z99166: through the backend, a superseded Open capture answers null and keeps the opened capture", async () => {
    const inputs = [new FakeInput(), new FakeInput(), new FakeInput()];
    const b = await createWebBackend({
      wasm: w,
      usb: null,
      hid: null,
      serial: null,
      pickDirectory: null,
      crossOriginIsolated: false,
      pageEvents: null,
      sqlJs: loadSqlJs,
      download: () => {},
      pickPdCapture: browserPickPdFile(PDCAP_ACCEPT, docOf(inputs)),
      pickVendorPdLog: async () => null,
    } as unknown as Options);

    const opening = b.pdOpen();
    await Promise.resolve();
    (inputs[0] as FakeInput).files = [picked("pd-capture.pdcap", fixture("pd-capture.pdcap"))];
    (inputs[0] as FakeInput).fire("change");
    const opened = await opening;
    expect(opened?.rows.length).toBe(EXPECTED.pdcap.opened.rows.length);

    // A picker that never answers, then a new click.
    const stuck = b.pdOpen();
    for (let i = 0; i < 5; i++) await Promise.resolve();
    expect((inputs[1] as FakeInput).clicks).toBe(1);
    const again = b.pdOpen();
    for (let i = 0; i < 5; i++) await Promise.resolve();
    expect((inputs[2] as FakeInput).clicks).toBe(1);
    expect(await settledNow(stuck), "the stuck pick resolves once a new one starts").toBe(true);
    expect(await stuck).toBeNull();

    (inputs[2] as FakeInput).fire("cancel");
    expect(await again).toBeNull();
    const row = opened?.rows[1] as PdRow;
    expect(await b.pdDetails(row.index, true)).toEqual(EXPECTED.pdcap.details[1]);
  });
});

describe("the desktop's view of the fixtures (AC-EAKFEC.2, .3)", () => {
  it("a pdcap file opens with the desktop's rows, status, counts, details and cable", async () => {
    const b = await backendPicking([picked("pd-capture.pdcap", fixture("pd-capture.pdcap"))]);
    const opened = await b.pdOpen();
    expect(opened?.path).toBe("pd-capture.pdcap");
    expect(await seen(b, opened)).toEqual(EXPECTED.pdcap);
    expect(opened?.damaged_line).not.toBeNull();
  });

  it("a vendor PD log imports with the desktop's rows, status, counts, details and cable", async () => {
    const b = await backendPicking([], [picked("pd-vendor.sqlite", fixture("pd-vendor.sqlite"))]);
    const opened = await b.pdImport();
    expect(opened?.path).toBe("pd-vendor.sqlite");
    expect(await seen(b, opened)).toEqual(EXPECTED.vendor);
    expect(opened).toMatchObject({ oversized: 1, skipped: 3, truncated: false });
  });
});

describe("refusals (AC-EAKFEC.4)", () => {
  it("a file without a pdcap header is refused in the desktop's words, the open capture stays", async () => {
    const b = await backendPicking([
      picked("pd-capture.pdcap", fixture("pd-capture.pdcap")),
      picked("bad.pdcap", new TextEncoder().encode("not a pdcap file\n")),
    ]);
    await b.pdOpen();
    const e = await rejection(b.pdOpen());
    expect(e.kind).toBe("usage");
    expect(e.message).toMatch(/^Could not open the PD capture: /);
    expect(await b.pdDetails(0, true)).toEqual(EXPECTED.pdcap.details[0]);
  });

  it("a database with no pd_table is refused as no vendor PD log", async () => {
    const b = await backendPicking(
      [],
      [picked("other.sqlite", dbBytes("create table t(x); insert into t values(1);"))],
    );
    const e = await rejection(b.pdImport());
    expect(e).toEqual({
      kind: "usage",
      message:
        "Could not import the vendor PD log: the file is not a vendor PD log (it has no pd_table)",
      hint: null,
    });
  });

  it("a file that is no database is refused as unreadable, the open capture stays", async () => {
    const b = await backendPicking(
      [picked("pd-capture.pdcap", fixture("pd-capture.pdcap"))],
      [
        picked(
          "junk.sqlite",
          new TextEncoder().encode("this is no database at all, not even close"),
        ),
      ],
    );
    await b.pdOpen();
    const e = await rejection(b.pdImport());
    expect(e.kind).toBe("usage");
    expect(e.message).toMatch(
      /^Could not import the vendor PD log: could not read the vendor PD log: /,
    );
    expect(await b.pdCableSeen()).toEqual(EXPECTED.pdcap.cable);
  });
});

describe("the opened capture with no meter (AC-EAKFEC.5)", () => {
  it("answers details, cable, Clear and release, also through the per-meter commands", async () => {
    const b = await backendPicking([picked("pd-capture.pdcap", fixture("pd-capture.pdcap"))]);
    expect(await b.pdCableSeen()).toBeNull();
    await b.pdOpen();
    const meter = b.meter("any");
    expect(await meter.pdDetails(2, true)).toEqual(EXPECTED.pdcap.details[2]);
    expect(await meter.pdCableSeen()).toEqual(EXPECTED.pdcap.cable);
    expect(await b.pdClear()).toEqual({ next_index: 0, status_floor_ms: 0 });
    expect((await rejection(b.pdDetails(0, true))).message).toBe(
      "That PD record is not in the opened capture.",
    );
    expect(await b.pdCableSeen()).toBeNull();
  });

  it("release lets the opened capture go", async () => {
    const b = await backendPicking([], [picked("pd-vendor.sqlite", fixture("pd-vendor.sqlite"))]);
    await b.pdImport();
    expect(await b.pdCableSeen()).toEqual(EXPECTED.vendor.cable);
    await b.pdReleaseOpened();
    expect(await b.pdCableSeen()).toBeNull();
    expect((await rejection(b.pdDetails(0, true))).kind).toBe("usage");
  });
});

describe("save and open again in the browser (AC-EAKFEC.6)", () => {
  it("a capture saved with Save capture opens with the rows it was saved with", async () => {
    vi.useFakeTimers({
      toFake: ["setTimeout", "clearTimeout", "setInterval", "clearInterval", "Date"],
    });
    vi.setSystemTime(1_790_000_000_000);
    const nav = new FakeUsbNavigator();
    nav.devices = [realMeterDevice(w, { serial: "123456", scenario: "pd-9v-2a" })];
    const sink = downloadSink();
    const saved: Picked[] = [];
    const b = await settle(
      backendPicking([], [], {
        usb: nav,
        download: sink.download,
        pickPdCapture: async () => saved.shift() ?? null,
      }),
    );
    const id = (await settle(b.listDevices())).find((d) => d.serial === "123456")?.id;
    if (!id) throw new Error("the permitted meter is not listed");
    const events: BackendEvent[] = [];
    await settle(b.connect(id, 50, (e) => events.push(e)));
    await settle(b.pdStart());
    await vi.advanceTimersByTimeAsync(3_000);
    await settle(b.pdStop());
    const live: PdRow[] = events.flatMap((e) => (e.type === "pd_rows" ? e.data.rows : []));
    expect(live.length).toBeGreaterThan(0);
    const result = await settle(b.pdSave("capture.pdcap"));
    const file = sink.downloads[0] as Download;
    saved.push(picked("capture.pdcap", new Uint8Array(await file.blob.arrayBuffer())));

    const opened = await settle(b.pdOpen());
    expect(opened?.rows.length).toBe(result?.rows);
    expect(opened?.rows.map((r) => [r.name, r.summary, r.elapsed_ms])).toEqual(
      live.map((r) => [r.name, r.summary, r.elapsed_ms]),
    );
    expect(opened?.damaged_line).toBeNull();
    const caps = opened?.rows.find((r) => r.name === "Source_Capabilities") as PdRow;
    const fromFile = await settle(b.pdDetails(caps.index, true));
    const fromLive = await settle(b.pdDetails(caps.index));
    expect(fromFile).toEqual(fromLive);
    await settle(b.disconnect());
  });
});
