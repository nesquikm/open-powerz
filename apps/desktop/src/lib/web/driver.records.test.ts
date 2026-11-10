/**
 * FR W8SHX8: meter settings and offline records over the browser session driver.
 *
 * - AC-W8SHX8.1: `readSettings` sends only GET_DATA SETTINGS and gives the rows the wasm
 *   core (the desktop decoder) makes of the reply.
 * - AC-W8SHX8.2: `listRecords` gives the demo meter's three records; it authenticates at
 *   level 1 first when the session is below it.
 * - AC-W8SHX8.3: `downloadRecord` reads only that record's listed window, in reads of at
 *   most 256 bytes; an unlisted record is refused before any memory read with the desktop
 *   wording; a request the wasm core forbids is refused as unsafe.
 * - AC-W8SHX8.4: the downloaded record equals what the wasm core decodes from the same
 *   replies (index, name, interval, samples).
 * - AC-W8SHX8.6: a HID session refuses all three with the basic-readings wording; a
 *   session whose sign-in failed refuses the list and the download with the desktop
 *   reason, while the settings read still works.
 *
 * FR CJ3QCB: every record list starts with a fresh level-1 sign-in (AC-CJ3QCB.1); a
 * refused sign-in fails it with the sign-in message (AC-CJ3QCB.2); the download keeps
 * working on the list it read (AC-CJ3QCB.3).
 *
 * Runs against the wasm demo meter on Vitest's fake clock.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { MeterSettings, OfflineRecord, RecordList, UiError } from "../backend";
import { DemoTransport } from "./demoTransport";
import { MeterDriver } from "./driver";
import type { PowerzWasm } from "./driver";
import {
  classify,
  loadWasm,
  recordWasm,
  type FrameName,
  type RecordBookLike,
} from "./testWasm.test";
import type { WebTransport } from "./transport";

type DriverOptions = Parameters<typeof MeterDriver.connect>[0];

/** The driver with the FR W8SHX8 commands. */
type RecordsDriver = MeterDriver & {
  readSettings(): Promise<MeterSettings>;
  listRecords(): Promise<RecordList>;
  downloadRecord(index: number): Promise<OfflineRecord>;
};

/** Records every frame with its reply; can spoil every auth reply. */
class Recorder implements WebTransport {
  log: { frame: Uint8Array; reply: Uint8Array | null }[] = [];
  closed = false;
  spoilAuth = false;
  constructor(
    private w: PowerzWasm,
    private inner: WebTransport,
  ) {}
  names(from = 0): FrameName[] {
    return this.log.slice(from).map((e) => classify(this.w, e.frame));
  }
  async transact(frame: Uint8Array, timeoutMs: number): Promise<Uint8Array> {
    const entry = { frame: frame.slice(), reply: null as Uint8Array | null };
    this.log.push(entry);
    const reply = await this.inner.transact(frame, timeoutMs);
    if (this.spoilAuth && classify(this.w, frame) === "auth") {
      const bad = reply.slice();
      for (let i = 4; i < bad.length; i++) bad[i] = (bad[i] ?? 0) ^ 0xa5;
      entry.reply = bad;
      return bad;
    }
    entry.reply = reply.slice();
    return reply;
  }
  read(timeoutMs: number): Promise<Uint8Array> {
    return this.inner.read(timeoutMs);
  }
  async close(): Promise<void> {
    this.closed = true;
    await this.inner.close();
  }
}

let w: PowerzWasm;

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

async function settle<T>(p: Promise<T>, maxMs = 10_000): Promise<T> {
  let done = false;
  let value: T | undefined;
  let error: unknown;
  let failed = false;
  p.then(
    (v) => {
      done = true;
      value = v;
    },
    (e: unknown) => {
      done = true;
      failed = true;
      error = e;
    },
  );
  for (let i = 0; i < 5 && !done; i++) await Promise.resolve();
  for (let t = 0; t < maxMs && !done; t += 10) await vi.advanceTimersByTimeAsync(10);
  if (!done) throw new Error("promise did not settle on the fake clock");
  if (failed) throw error;
  return value as T;
}

async function rejection(p: Promise<unknown>): Promise<UiError> {
  try {
    await settle(p);
  } catch (e) {
    return e as UiError;
  }
  throw new Error("the call resolved");
}

interface Run {
  rec: Recorder;
  driver: RecordsDriver;
  errors: UiError[];
}

async function start(
  o: { hid?: boolean; spoilAuth?: boolean; wasm?: PowerzWasm } = {},
): Promise<Run> {
  const wasm = o.wasm ?? w;
  const rec = new Recorder(wasm, new DemoTransport(wasm, "load-5v-1a", () => Date.now()));
  rec.spoilAuth = o.spoilAuth === true;
  const errors: UiError[] = [];
  const options = {
    wasm,
    transport: rec,
    rateCode: 2,
    hid: o.hid === true,
    onBatch: () => {},
    onError: (e: UiError) => errors.push(e),
    random: () => 0x1234_5678_9abc_def0n,
  } as unknown as DriverOptions;
  const driver = (await settle(MeterDriver.connect(options))) as RecordsDriver;
  await vi.advanceTimersByTimeAsync(250);
  return { rec, driver, errors };
}

/** Frames other than the stream's own polls. */
const POLLS: readonly FrameName[] = ["get_data:adc", "get_data:adc_queue"];
const noPolls = (names: FrameName[]) => names.filter((n) => !POLLS.includes(n));

/** The reply the recorder saw for the last frame named `name` since `from`. */
function lastReply(rec: Recorder, name: FrameName, from = 0): Uint8Array {
  const entries = rec.log.slice(from).filter((e) => classify(w, e.frame) === name);
  const reply = entries.at(-1)?.reply;
  if (!reply) throw new Error(`no reply for ${name}`);
  return reply;
}

const NOT_SAFE = "The app refused a request that is not safe.";
const SIGN_IN: UiError = {
  kind: "protocol",
  message: "The meter did not accept the app's sign-in.",
  hint: "Replug the meter and try again.",
};
const HID_WORDING = /^.+ is not available over HID: it gives basic readings only\.$/;

describe("settings read (AC-W8SHX8.1)", () => {
  it("sends only GET_DATA SETTINGS and gives the core's rows", async () => {
    const { rec, driver, errors } = await start();
    const from = rec.log.length;
    const settings = await settle(driver.readSettings());
    expect(noPolls(rec.names(from))).toEqual(["get_data:settings"]);
    const decoded = recordWasm(w).decodeSettings(lastReply(rec, "get_data:settings", from));
    expect(settings.fields).toEqual(decoded.fields);
    expect(settings.fields.length).toBeGreaterThan(5);
    const keys = settings.fields.map((f) => f.key);
    expect(keys).toContain("brightness");
    for (const f of settings.fields) {
      expect(typeof f.label).toBe("string");
      expect(typeof f.value).toBe("string");
    }
    expect(errors).toEqual([]);
    await settle(driver.disconnect());
  });
});

describe("record list (AC-W8SHX8.2)", () => {
  it("lists the demo meter's records as the core decodes them, with no extra frames", async () => {
    const { rec, driver } = await start();
    expect(driver.authLevel).toBeGreaterThanOrEqual(1);
    const from = rec.log.length;
    const list = await settle(driver.listRecords());
    // AC-CJ3QCB.1: a fresh sign-in before the list, also when the session holds one.
    expect(noPolls(rec.names(from))).toEqual(["auth", "get_data:record_list"]);
    expect(list.records.map((r) => [r.index, r.name, r.interval_s, r.points])).toEqual([
      [1, "PHONE_5V", 1, 300],
      [2, "PD_9V_TEST", 2, 120],
      [3, "LAPTOP_20V", 5, 60],
    ]);
    const book = new (recordWasm(w).RecordBook)();
    expect(list).toEqual(book.decodeList(lastReply(rec, "get_data:record_list", from)));
    book.free();
    for (const r of list.records) {
      for (const key of ["duration_s", "charge_ah", "energy_wh", "offset"] as const) {
        expect(typeof r[key], key).toBe("number");
      }
    }
    // PHONE_5V stores negative totals (one current direction).
    expect(list.records[0]?.charge_ah).toBeLessThan(0);
    await settle(driver.disconnect());
  });

  it("authenticates at level 1 before the list when the session is below it", async () => {
    const { rec, driver } = await start({ spoilAuth: true });
    expect(driver.authLevel).toBe(0);
    rec.spoilAuth = false;
    const from = rec.log.length;
    const list = await settle(driver.listRecords());
    const names = noPolls(rec.names(from));
    expect(names).toContain("auth");
    expect(names.at(-1)).toBe("get_data:record_list");
    expect(names.indexOf("auth")).toBeLessThan(names.indexOf("get_data:record_list"));
    expect(names.filter((n) => n === "get_data:record_list")).toHaveLength(1);
    expect(list.records).toHaveLength(3);
    expect(driver.authLevel).toBeGreaterThanOrEqual(1);
    await settle(driver.disconnect());
  });
});

describe("record download (AC-W8SHX8.3, AC-W8SHX8.4)", () => {
  it("reads only that record's window in reads of at most 256 bytes", async () => {
    const { rec, driver } = await start();
    await settle(driver.listRecords());
    const from = rec.log.length;
    const record = await settle(driver.downloadRecord(2));
    const names = noPolls(rec.names(from));
    const reads = names.filter((n) => n.startsWith("record_read:") || n.startsWith("memory_read:"));
    // PD_9V_TEST: 120 points × 16 bytes = 1920 bytes = 8 reads of ≤ 256 bytes.
    expect(reads).toEqual(Array(8).fill("record_read:2"));
    expect(names.slice(0, 2)).toEqual(["auth", "get_data:record_list"]);
    expect(
      names.every((n) => n === "record_read:2" || n === "get_data:record_list" || n === "auth"),
    ).toBe(true);
    expect(record.index).toBe(2);
    expect(record.name).toBe("PD_9V_TEST");
    expect(record.interval_s).toBe(2);
    expect(record.samples).toHaveLength(120);
    await settle(driver.disconnect());
  });

  it("equals the core's decoded record for the same replies", async () => {
    const { rec, driver } = await start();
    const listFrom = rec.log.length;
    await settle(driver.listRecords());
    const from = rec.log.length;
    const record = await settle(driver.downloadRecord(1));
    // The window comes from the latest list the driver read.
    const listReply = lastReply(rec, "get_data:record_list", listFrom);
    const book: RecordBookLike = new (recordWasm(w).RecordBook)();
    book.decodeList(listReply);
    const chunkReplies = rec.log
      .slice(from)
      .filter((e) => classify(w, e.frame) === "record_read:1")
      .map((e) => e.reply as Uint8Array);
    const chunks = book.chunks(1);
    expect(chunkReplies).toHaveLength(chunks.length);
    const parts = chunks.map((c, i) =>
      w.decodeMemoryRead(chunkReplies[i] as Uint8Array, c.addr, c.len),
    );
    const data = new Uint8Array(parts.reduce((n, p) => n + p.length, 0));
    let at = 0;
    for (const p of parts) {
      data.set(p, at);
      at += p.length;
    }
    expect(record).toEqual(book.decodeRecord(1, data));
    book.free();
    expect(record.samples).toHaveLength(300);
    expect(record.samples[0]?.t_s).toBe(0);
    expect(record.samples[1]?.t_s).toBe(1);
    expect(record.samples[0]?.vbus_v).toBeCloseTo(5.1, 1);
    expect(record.samples[10]?.ibus_a).toBeLessThan(0);
    await settle(driver.disconnect());
  });

  it("refuses an unlisted record with the desktop wording before any memory read", async () => {
    const { rec, driver } = await start();
    await settle(driver.listRecords());
    for (const index of [0, 4, 99]) {
      const from = rec.log.length;
      const error = await rejection(driver.downloadRecord(index));
      expect(error).toMatchObject({ kind: "usage", message: "That record is not on the meter." });
      const names = noPolls(rec.names(from));
      // AC-CJ3QCB.1: the download's list starts with a fresh sign-in; no memory read follows.
      expect(names, `index ${index}`).toEqual(["auth", "get_data:record_list"]);
    }
    await settle(driver.disconnect());
  });

  it("refuses a read the core forbids as unsafe, sending no record read", async () => {
    const real = recordWasm(w).RecordBook;
    /** A RecordBook whose window guard refuses every chunk. */
    class ForbiddingBook {
      private inner = new real();
      listRequest(id: number) {
        return this.inner.listRequest(id);
      }
      decodeList(reply: Uint8Array) {
        return this.inner.decodeList(reply);
      }
      chunks(index: number) {
        return this.inner.chunks(index);
      }
      chunkRequest(): Uint8Array {
        throw { kind: "forbidden", message: "memory read outside the listed windows" };
      }
      decodeRecord(index: number, data: Uint8Array) {
        return this.inner.decodeRecord(index, data);
      }
      free() {
        this.inner.free();
      }
    }
    const wasm = { ...w, RecordBook: ForbiddingBook } as unknown as PowerzWasm;
    const { rec, driver, errors } = await start({ wasm });
    await settle(driver.listRecords());
    const from = rec.log.length;
    const error = await rejection(driver.downloadRecord(1));
    expect(error).toMatchObject({ kind: "usage", message: NOT_SAFE });
    expect(rec.log.slice(from).some((e) => e.frame[0] === 0x44)).toBe(false);
    // The session goes on.
    expect(errors).toEqual([]);
    await settle(driver.disconnect());
  });

  it("passes a core usage refusal's message through", async () => {
    const real = recordWasm(w).RecordBook;
    class EmptyRecordBook {
      private inner = new real();
      listRequest(id: number) {
        return this.inner.listRequest(id);
      }
      decodeList(reply: Uint8Array) {
        return this.inner.decodeList(reply);
      }
      chunks(): never {
        throw { kind: "usage", message: "That record has no samples." };
      }
      chunkRequest(id: number, addr: number, len: number) {
        return this.inner.chunkRequest(id, addr, len);
      }
      decodeRecord(index: number, data: Uint8Array) {
        return this.inner.decodeRecord(index, data);
      }
      free() {
        this.inner.free();
      }
    }
    const wasm = { ...w, RecordBook: EmptyRecordBook } as unknown as PowerzWasm;
    const { rec, driver } = await start({ wasm });
    await settle(driver.listRecords());
    const from = rec.log.length;
    const error = await rejection(driver.downloadRecord(3));
    expect(error).toMatchObject({ kind: "usage", message: "That record has no samples." });
    expect(rec.log.slice(from).some((e) => e.frame[0] === 0x44)).toBe(false);
    await settle(driver.disconnect());
  });
});

describe("refusals (AC-W8SHX8.6)", () => {
  it("a HID session refuses all three with the basic-readings wording and no traffic", async () => {
    const { rec, driver } = await start({ hid: true });
    const from = rec.log.length;
    for (const call of [
      () => driver.readSettings(),
      () => driver.listRecords(),
      () => driver.downloadRecord(1),
    ]) {
      const error = await rejection(call());
      expect(error.kind).toBe("usage");
      expect(error.message).toMatch(HID_WORDING);
    }
    expect(rec.names(from).filter((n) => n !== "get_data:adc")).toEqual([]);
    await settle(driver.disconnect());
  });

  it("a session whose sign-in failed refuses the list and download, but reads settings", async () => {
    const { rec, driver, errors } = await start({ spoilAuth: true });
    expect(driver.authLevel).toBe(0);
    expect(driver.mode).toBe("snapshot");

    let from = rec.log.length;
    expect(await rejection(driver.listRecords())).toEqual(SIGN_IN);
    let names = noPolls(rec.names(from));
    expect(names).toContain("auth");
    expect(names).not.toContain("get_data:record_list");

    from = rec.log.length;
    expect(await rejection(driver.downloadRecord(1))).toEqual(SIGN_IN);
    names = noPolls(rec.names(from));
    expect(names.some((n) => n.startsWith("record_read:"))).toBe(false);
    expect(names).not.toContain("get_data:record_list");

    from = rec.log.length;
    const settings = await settle(driver.readSettings());
    expect(noPolls(rec.names(from))).toEqual(["get_data:settings"]);
    expect(settings.fields.length).toBeGreaterThan(5);
    expect(errors).toEqual([]);
    await settle(driver.disconnect());
  });
});

describe("hardening (FR W8SHX8)", () => {
  it("a disconnect during a download rejects it as disconnected, not a freed-book error", async () => {
    const { driver } = await start();
    const download = driver.downloadRecord(1);
    const gone = driver.disconnect();
    const error = await rejection(download);
    await settle(gone);
    expect(error.kind).toBe("disconnected");
  });

  it("a memory read whose next transfer comes back empty fails instead of reading forever", async () => {
    const { rec, driver } = await start();
    const inner = rec.read.bind(rec);
    let cut = false;
    const transact = rec.transact.bind(rec);
    rec.transact = async (frame, timeoutMs) => {
      const reply = await transact(frame, timeoutMs);
      if (classify(w, frame).startsWith("record_read")) {
        cut = true;
        return reply.slice(0, 20);
      }
      return reply;
    };
    rec.read = (timeoutMs) => (cut ? Promise.resolve(new Uint8Array(0)) : inner(timeoutMs));
    const error = await rejection(driver.downloadRecord(1));
    expect(error.kind).toBe("protocol");
    expect(error.message).toMatch(/ended early/);
    await settle(driver.disconnect());
  });
});

describe("a fresh sign-in before every list (FR CJ3QCB)", () => {
  it("AC-CJ3QCB.1: each list on a signed-in session sends the sign-in, then the list", async () => {
    const { rec, driver, errors } = await start();
    expect(driver.authLevel).toBeGreaterThanOrEqual(1);
    for (let i = 0; i < 3; i++) {
      const from = rec.log.length;
      const list = await settle(driver.listRecords());
      expect(noPolls(rec.names(from)), `list ${i + 1}`).toEqual(["auth", "get_data:record_list"]);
      expect(list.records.map((r) => r.name)).toEqual(["PHONE_5V", "PD_9V_TEST", "LAPTOP_20V"]);
      expect(driver.authLevel).toBeGreaterThanOrEqual(1);
    }
    expect(errors).toEqual([]);
    await settle(driver.disconnect());
  });

  it("AC-CJ3QCB.2: a refused sign-in on a signed-in session fails the list, sending no list", async () => {
    const { rec, driver, errors } = await start();
    await settle(driver.listRecords());
    rec.spoilAuth = true;
    let from = rec.log.length;
    expect(await rejection(driver.listRecords())).toEqual(SIGN_IN);
    let names = noPolls(rec.names(from));
    expect(names).toEqual(["auth"]);

    from = rec.log.length;
    expect(await rejection(driver.downloadRecord(1))).toEqual(SIGN_IN);
    names = noPolls(rec.names(from));
    expect(names).not.toContain("get_data:record_list");
    expect(names.some((n) => n.startsWith("record_read:"))).toBe(false);

    // The meter signs in again: the list works once more.
    rec.spoilAuth = false;
    from = rec.log.length;
    const list = await settle(driver.listRecords());
    expect(noPolls(rec.names(from))).toEqual(["auth", "get_data:record_list"]);
    expect(list.records).toHaveLength(3);
    expect(errors).toEqual([]);
    await settle(driver.disconnect());
  });

  it("AC-CJ3QCB.3: a download signs in, lists, and reads that list's window", async () => {
    const { rec, driver } = await start();
    await settle(driver.listRecords());
    const from = rec.log.length;
    const record = await settle(driver.downloadRecord(3));
    const names = noPolls(rec.names(from));
    // LAPTOP_20V: 60 points × 16 bytes = 960 bytes = 4 reads.
    expect(names).toEqual([
      "auth",
      "get_data:record_list",
      ...Array<FrameName>(4).fill("record_read:3"),
    ]);
    expect(record).toMatchObject({ index: 3, name: "LAPTOP_20V", interval_s: 5 });
    expect(record.samples).toHaveLength(60);
    await settle(driver.disconnect());
  });
});
