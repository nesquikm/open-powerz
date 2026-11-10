/**
 * FR W8SHXA: deleting offline records over the browser session driver.
 *
 * - AC-W8SHXA.1: `deleteRecords` reads the list again first; the typed confirmation
 *   must be the record's name (or "all"), else nothing is downloaded, saved or written.
 * - AC-W8SHXA.2: every affected record is downloaded and its backup (the wasm core's
 *   desktop file name and CSV) handed to `saveBackup` before the table write; a failed
 *   download saves and writes nothing.
 * - AC-W8SHXA.3: the one table write is the frame the core builds from the first list;
 *   a too-long table is refused before any download; "all" on an empty list sends
 *   nothing.
 * - AC-W8SHXA.4: ACCEPT / FINISHED count; a REJECT fails; the list is read again and
 *   must match, else the delete fails naming the backup (" Backup: …"); no retry.
 * - AC-W8SHXA.5: the answer is `{ records, backup_dir, backups }`.
 * - AC-W8SHXA.6: no SET_DATA but the record-table write is sent; a HID session and a
 *   session whose sign-in failed refuse the delete.
 *
 * FR CJ3QCB: both of the delete's lists start with a fresh sign-in, and the delete keeps
 * working on the list it read, also against a meter that answers the list from its state
 * at the last sign-in (AC-CJ3QCB.1, .3).
 *
 * Runs against the wasm demo meter on Vitest's fake clock.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { RecordDeleteResult, UiError } from "../backend";
import { DemoTransport } from "./demoTransport";
import { MeterDriver } from "./driver";
import type { PowerzWasm } from "./driver";
import {
  classify,
  loadWasm,
  recordWasm,
  type FrameName,
  type RecordDeleteBookLike,
} from "./testWasm.test";
import type { WebTransport } from "./transport";

type DriverOptions = Parameters<typeof MeterDriver.connect>[0];

/** The driver with the FR W8SHXA command. */
type DeleteDriver = MeterDriver & {
  listRecords(): Promise<{ records: { index: number; name: string }[] }>;
  deleteRecords(
    target: number | "all",
    confirm: string,
    saveBackup: (name: string, csv: string) => void,
  ): Promise<RecordDeleteResult>;
};

type Rewrite = (frame: Uint8Array, reply: Uint8Array, name: FrameName) => Uint8Array | null;

/** Records every frame with its reply; can spoil auth replies and rewrite others. */
class Recorder implements WebTransport {
  log: { frame: Uint8Array; reply: Uint8Array | null; name: FrameName }[] = [];
  spoilAuth = false;
  /** Rewrites a reply (`null` keeps it); `swallow` answers without the demo meter. */
  rewrite: Rewrite | null = null;
  swallow: ((frame: Uint8Array, name: FrameName) => Uint8Array | null) | null = null;
  constructor(
    private w: PowerzWasm,
    private inner: WebTransport,
  ) {}
  names(from = 0): FrameName[] {
    return this.log.slice(from).map((e) => e.name);
  }
  async transact(frame: Uint8Array, timeoutMs: number): Promise<Uint8Array> {
    const name = classify(this.w, frame);
    const entry = { frame: frame.slice(), reply: null as Uint8Array | null, name };
    this.log.push(entry);
    const swallowed = this.swallow?.(frame, name) ?? null;
    let reply = swallowed ?? (await this.inner.transact(frame, timeoutMs));
    if (this.spoilAuth && name === "auth") {
      const bad = reply.slice();
      for (let i = 4; i < bad.length; i++) bad[i] = (bad[i] ?? 0) ^ 0xa5;
      reply = bad;
    }
    reply = this.rewrite?.(frame, reply, name) ?? reply;
    entry.reply = reply.slice();
    return reply;
  }
  read(timeoutMs: number): Promise<Uint8Array> {
    return this.inner.read(timeoutMs);
  }
  async close(): Promise<void> {
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

async function settle<T>(p: Promise<T>, maxMs = 20_000): Promise<T> {
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
  driver: DeleteDriver;
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
  const driver = (await settle(MeterDriver.connect(options))) as DeleteDriver;
  await vi.advanceTimersByTimeAsync(250);
  return { rec, driver, errors };
}

const POLLS: readonly FrameName[] = ["get_data:adc", "get_data:adc_queue"];
const noPolls = (names: FrameName[]) => names.filter((n) => !POLLS.includes(n));

/** The saveBackup sink: each call with the frames sent so far. */
function sink(rec: Recorder) {
  const saved: { name: string; csv: string; writesBefore: number }[] = [];
  const save = (name: string, csv: string) => {
    saved.push({
      name,
      csv,
      writesBefore: rec.log.filter((e) => e.name === "record_table_write").length,
    });
  };
  return { saved, save };
}

const DEMO = ["PHONE_5V", "PD_9V_TEST", "LAPTOP_20V"];
const BACKUP_DIR = "your browser's downloads";
const BACKUP_SUFFIX = ` Backup: ${BACKUP_DIR}`;
const NOT_LISTED = "That record is not on the meter.";
const CONFIRM_MISMATCH = "The typed confirmation does not match; nothing was deleted.";
const SIGN_IN: UiError = {
  kind: "protocol",
  message: "The meter did not accept the app's sign-in.",
  hint: "Replug the meter and try again.",
};

const tableWrites = (rec: Recorder, from = 0) =>
  rec.log.slice(from).filter((e) => e.name === "record_table_write");
const recordReads = (names: FrameName[]) => names.filter((n) => n.startsWith("record_read:"));

/** What the wasm core makes of the replies the driver read since `from`: the first
 * list's book, the write it would build, and record `index`'s backup. */
function coreView(rec: Recorder, from: number) {
  const entries = rec.log.slice(from);
  const firstList = entries.find((e) => e.name === "get_data:record_list");
  if (!firstList?.reply) throw new Error("no record list");
  const book = new (recordWasm(w).RecordBook)() as RecordDeleteBookLike;
  book.decodeList(firstList.reply);
  const backup = (index: number) => {
    const replies = entries
      .filter((e) => e.name === `record_read:${index}`)
      .map((e) => e.reply as Uint8Array);
    const chunks = book.chunks(index);
    const parts = chunks.map((c, i) => w.decodeMemoryRead(replies[i] as Uint8Array, c.addr, c.len));
    const data = new Uint8Array(parts.reduce((n, p) => n + p.length, 0));
    let at = 0;
    for (const p of parts) {
      data.set(p, at);
      at += p.length;
    }
    return book.backupFile(index, data);
  };
  return { book, backup };
}

describe("one record (AC-W8SHXA.1, .2, .3, .5)", () => {
  it("lists, downloads, saves the backup, writes the table once and lists again", async () => {
    const { rec, driver, errors } = await start();
    const from = rec.log.length;
    const { saved, save } = sink(rec);
    const result = await settle(driver.deleteRecords(2, "PD_9V_TEST", save));

    const names = noPolls(rec.names(from));
    // AC-CJ3QCB.1: each of the two lists starts with a fresh sign-in.
    expect(names).toEqual([
      "auth",
      "get_data:record_list",
      ...Array<FrameName>(8).fill("record_read:2"),
      "record_table_write",
      "auth",
      "get_data:record_list",
    ]);

    // The backup is the core's desktop file, saved before the write.
    const { book, backup } = coreView(rec, from);
    const want = backup(2);
    expect(want.name).toBe("02_PD_9V_TEST.csv");
    expect(saved).toEqual([{ name: want.name, csv: want.csv, writesBefore: 0 }]);

    // The write is the core's frame for the kept entries, unchanged.
    const [write] = tableWrites(rec, from);
    if (!write) throw new Error("no table write");
    expect(Array.from(write.frame)).toEqual(
      Array.from(book.deleteRequest(write.frame[1] ?? 0, new Uint32Array([2]))),
    );
    book.free();

    expect(result.records.map((r) => [r.index, r.name])).toEqual([
      [1, "PHONE_5V"],
      [2, "LAPTOP_20V"],
    ]);
    expect(result.backup_dir).toBe(BACKUP_DIR);
    expect(result.backups).toEqual([want.name]);
    expect(Object.keys(result).sort()).toEqual(["backup_dir", "backups", "records"]);
    expect((await settle(driver.listRecords())).records.map((r) => r.name)).toEqual([
      "PHONE_5V",
      "LAPTOP_20V",
    ]);
    expect(errors).toEqual([]);
    await settle(driver.disconnect());
  });

  it("deletes every record with all, every backup saved before the one write", async () => {
    const { rec, driver } = await start();
    const from = rec.log.length;
    const { saved, save } = sink(rec);
    const result = await settle(driver.deleteRecords("all", "all", save));
    const names = noPolls(rec.names(from));
    expect(names.filter((n) => n === "record_table_write")).toHaveLength(1);
    expect(names.at(-1)).toBe("get_data:record_list");
    const writeAt = names.indexOf("record_table_write");
    for (const i of [1, 2, 3]) {
      const reads = names.flatMap((n, at) => (n === `record_read:${i}` ? [at] : []));
      expect(reads.length, `record ${i}`).toBeGreaterThan(0);
      expect(Math.max(...reads)).toBeLessThan(writeAt);
    }
    const { backup } = coreView(rec, from);
    const want = [1, 2, 3].map(backup);
    expect(saved).toEqual(want.map((f) => ({ name: f.name, csv: f.csv, writesBefore: 0 })));
    expect(result).toEqual({
      records: [],
      backup_dir: BACKUP_DIR,
      backups: ["01_PHONE_5V.csv", "02_PD_9V_TEST.csv", "03_LAPTOP_20V.csv"],
    });
    for (const s of saved) expect(s.csv.split("\n").length).toBeGreaterThan(10);
    await settle(driver.disconnect());
  });

  it('"all" on an empty list sends nothing but the list and saves nothing', async () => {
    const { rec, driver } = await start();
    await settle(driver.deleteRecords("all", "all", () => {}));
    const from = rec.log.length;
    const { saved, save } = sink(rec);
    const result = await settle(driver.deleteRecords("all", "all", save));
    expect(noPolls(rec.names(from))).toEqual(["auth", "get_data:record_list"]);
    expect(saved).toEqual([]);
    expect(result).toEqual({ records: [], backup_dir: BACKUP_DIR, backups: [] });
    await settle(driver.disconnect());
  });
});

describe("the typed confirmation (AC-W8SHXA.1)", () => {
  for (const [target, confirm] of [
    [1, "all"],
    [1, "phone_5v"],
    [1, "PD_9V_TEST"],
    [1, ""],
    ["all", "PHONE_5V"],
    ["all", "ALL"],
  ] as const) {
    it(`refuses ${String(target)} with "${confirm}" after a fresh list, saving and writing nothing`, async () => {
      const { rec, driver } = await start();
      const from = rec.log.length;
      const { saved, save } = sink(rec);
      const error = await rejection(driver.deleteRecords(target, confirm, save));
      expect(error).toMatchObject({ kind: "usage", message: CONFIRM_MISMATCH });
      expect(noPolls(rec.names(from))).toEqual(["auth", "get_data:record_list"]);
      expect(saved).toEqual([]);
      expect((await settle(driver.listRecords())).records.map((r) => r.name)).toEqual(DEMO);
      await settle(driver.disconnect());
    });
  }

  it("refuses an unlisted record with the desktop wording", async () => {
    const { rec, driver } = await start();
    for (const index of [0, 4, 99]) {
      const from = rec.log.length;
      const error = await rejection(driver.deleteRecords(index, "PHONE_5V", () => {}));
      expect(error).toMatchObject({ kind: "usage", message: NOT_LISTED });
      expect(noPolls(rec.names(from))).toEqual(["auth", "get_data:record_list"]);
    }
    await settle(driver.disconnect());
  });
});

describe("refusals before the backup (AC-W8SHXA.3)", () => {
  it("a table too long to write is refused before any download, save or write", async () => {
    const real = recordWasm(w).RecordBook;
    const TOO_LONG = {
      kind: "usage",
      message: "Deleting that would leave more records than one table write can hold.",
      hint: "Delete more records at once, or all of them.",
    };
    /** A RecordBook whose planner refuses as a too-long table. */
    class TooLongBook extends (real as unknown as new () => RecordDeleteBookLike) {
      deleteTargets(): never {
        throw TOO_LONG;
      }
    }
    const wasm = { ...w, RecordBook: TooLongBook } as unknown as PowerzWasm;
    const { rec, driver } = await start({ wasm });
    const from = rec.log.length;
    const { saved, save } = sink(rec);
    const error = await rejection(driver.deleteRecords(1, "PHONE_5V", save));
    expect(error).toMatchObject(TOO_LONG);
    expect(noPolls(rec.names(from))).toEqual(["auth", "get_data:record_list"]);
    expect(saved).toEqual([]);
    await settle(driver.disconnect());
  });

  it("a failed download saves nothing and writes nothing", async () => {
    const { rec, driver } = await start();
    // Record 3's reads are refused: records 1 and 2 were downloaded already.
    rec.rewrite = (frame, _reply, name) =>
      name === "record_read:3" ? new Uint8Array([0x06, frame[1] ?? 0, 0, 0]) : null;
    const from = rec.log.length;
    const { saved, save } = sink(rec);
    const error = await rejection(driver.deleteRecords("all", "all", save));
    expect(error.message.startsWith("The backup download failed, so nothing was deleted.")).toBe(
      true,
    );
    expect(error.message.length).toBeGreaterThan(
      "The backup download failed, so nothing was deleted.".length,
    );
    expect(saved).toEqual([]);
    expect(tableWrites(rec, from)).toEqual([]);
    rec.rewrite = null;
    expect((await settle(driver.listRecords())).records.map((r) => r.name)).toEqual(DEMO);
    await settle(driver.disconnect());
  });

  it("a save that throws writes nothing", async () => {
    const { rec, driver } = await start();
    const from = rec.log.length;
    const error = await rejection(
      driver.deleteRecords(1, "PHONE_5V", () => {
        throw new Error("the download was blocked");
      }),
    );
    expect(error.message).toMatch(/^The backup download failed, so nothing was deleted\./);
    expect(tableWrites(rec, from)).toEqual([]);
    await settle(driver.disconnect());
  });
});

describe("the write's reply and the re-read (AC-W8SHXA.4)", () => {
  it("a REJECT fails naming the backup, without a retry or a re-list", async () => {
    const { rec, driver } = await start();
    rec.swallow = (frame, name) =>
      name === "record_table_write" ? new Uint8Array([0x06, frame[1] ?? 0, 0, 0]) : null;
    const from = rec.log.length;
    const { saved, save } = sink(rec);
    const error = await rejection(driver.deleteRecords(1, "PHONE_5V", save));
    expect(error).toEqual({
      kind: "protocol",
      message: `The meter refused the request.${BACKUP_SUFFIX}`,
      hint: "Updating the meter's firmware may help.",
    });
    expect(saved.map((s) => s.name)).toEqual(["01_PHONE_5V.csv"]);
    expect(tableWrites(rec, from)).toHaveLength(1);
    const names = noPolls(rec.names(from));
    expect(names.at(-1)).toBe("record_table_write");
    await settle(driver.disconnect());
  });

  it("an ACCEPT counts as written", async () => {
    const { rec, driver } = await start();
    let forwarded = false;
    rec.rewrite = (frame, _reply, name) => {
      if (name !== "record_table_write") return null;
      forwarded = true;
      return new Uint8Array([0x05, frame[1] ?? 0, 0, 0]);
    };
    const result = await settle(driver.deleteRecords(3, "LAPTOP_20V", () => {}));
    expect(forwarded).toBe(true);
    expect(result.records.map((r) => r.name)).toEqual(["PHONE_5V", "PD_9V_TEST"]);
    await settle(driver.disconnect());
  });

  it("another reply fails naming the backup", async () => {
    const { rec, driver } = await start();
    rec.rewrite = (frame, _reply, name) =>
      name === "record_table_write" ? new Uint8Array([0x01, frame[1] ?? 0, 0, 0]) : null;
    const error = await rejection(driver.deleteRecords(1, "PHONE_5V", () => {}));
    expect(error.kind).toBe("protocol");
    expect(error.message.endsWith(BACKUP_SUFFIX)).toBe(true);
    expect(error.message).not.toMatch(/^The meter refused/);
    await settle(driver.disconnect());
  });

  it("a re-read that does not match fails naming the backup; nothing is retried", async () => {
    const { rec, driver } = await start();
    // The meter answers FINISHED but keeps its old table.
    rec.swallow = (frame, name) =>
      name === "record_table_write" ? new Uint8Array([0x07, frame[1] ?? 0, 0, 0]) : null;
    const from = rec.log.length;
    const { saved, save } = sink(rec);
    const error = await rejection(driver.deleteRecords(1, "PHONE_5V", save));
    expect(error).toEqual({
      kind: "protocol",
      message: `The record list does not match after the delete: the meter lists 3 records, expected 2.${BACKUP_SUFFIX}`,
      hint: "Read the list again; nothing was retried.",
    });
    expect(saved).toHaveLength(1);
    expect(tableWrites(rec, from)).toHaveLength(1);
    expect(noPolls(rec.names(from)).at(-1)).toBe("get_data:record_list");
    await settle(driver.disconnect());
  });
});

describe("only the record-table write, and only when signed in (AC-W8SHXA.6)", () => {
  it("every SET_DATA the delete sends is the record-table write", async () => {
    const { rec, driver } = await start();
    const from = rec.log.length;
    await settle(driver.deleteRecords(1, "PHONE_5V", () => {}));
    await settle(driver.deleteRecords("all", "all", () => {}));
    const setData = rec.log.slice(from).filter((e) => ((e.frame[0] ?? 0) & 0x7f) === 0x48);
    expect(setData.length).toBe(2);
    for (const e of setData) expect(e.name).toBe("record_table_write");
    expect(rec.names(from)).not.toContain("unknown");
    await settle(driver.disconnect());
  });

  it("a HID session refuses with the basic-readings wording and no traffic", async () => {
    const { rec, driver } = await start({ hid: true });
    const from = rec.log.length;
    const { saved, save } = sink(rec);
    const error = await rejection(driver.deleteRecords(1, "PHONE_5V", save));
    expect(error).toMatchObject({
      kind: "usage",
      message: "Deleting offline records is not available over HID: it gives basic readings only.",
    });
    expect(rec.names(from).filter((n) => n !== "get_data:adc")).toEqual([]);
    expect(saved).toEqual([]);
    await settle(driver.disconnect());
  });

  it("a session whose sign-in failed refuses with the desktop reason", async () => {
    const { rec, driver } = await start({ spoilAuth: true });
    expect(driver.authLevel).toBe(0);
    const from = rec.log.length;
    const { saved, save } = sink(rec);
    expect(await rejection(driver.deleteRecords("all", "all", save))).toEqual(SIGN_IN);
    const names = noPolls(rec.names(from));
    expect(names).not.toContain("get_data:record_list");
    expect(names).not.toContain("record_table_write");
    expect(recordReads(names)).toEqual([]);
    expect(saved).toEqual([]);
    await settle(driver.disconnect());
  });
});

/** A meter that answers RECORD_LIST from its state at the last sign-in (the inferred
 * device behaviour behind FR CJ3QCB): the first list after a sign-in is the real one,
 * later lists replay it until the next sign-in. */
function listFromLastSignIn(rec: Recorder): void {
  let snapshot: Uint8Array | null = null;
  rec.rewrite = (frame, reply, name) => {
    if (name === "auth") {
      snapshot = null;
      return null;
    }
    if (name !== "get_data:record_list") return null;
    if (!snapshot) {
      snapshot = reply.slice();
      return null;
    }
    const replay = snapshot.slice();
    replay[1] = frame[1] ?? 0;
    return replay;
  };
}

describe("a fresh sign-in before every list (FR CJ3QCB)", () => {
  it("AC-CJ3QCB.1: a list after a change since the last list shows the change", async () => {
    const { rec, driver, errors } = await start();
    listFromLastSignIn(rec);
    expect((await settle(driver.listRecords())).records.map((r) => r.name)).toEqual(DEMO);
    const result = await settle(driver.deleteRecords(2, "PD_9V_TEST", () => {}));
    expect(result.records.map((r) => r.name)).toEqual(["PHONE_5V", "LAPTOP_20V"]);
    expect((await settle(driver.listRecords())).records.map((r) => r.name)).toEqual([
      "PHONE_5V",
      "LAPTOP_20V",
    ]);
    expect(errors).toEqual([]);
    await settle(driver.disconnect());
  });

  it("AC-CJ3QCB.3: delete-all works on the list it read and backs up every record", async () => {
    const { rec, driver } = await start();
    listFromLastSignIn(rec);
    await settle(driver.listRecords());
    const from = rec.log.length;
    const { saved, save } = sink(rec);
    const result = await settle(driver.deleteRecords("all", "all", save));
    const names = noPolls(rec.names(from));
    expect(names.slice(0, 2)).toEqual(["auth", "get_data:record_list"]);
    expect(names.slice(-3)).toEqual(["record_table_write", "auth", "get_data:record_list"]);
    expect(saved.map((s) => s.name)).toEqual([
      "01_PHONE_5V.csv",
      "02_PD_9V_TEST.csv",
      "03_LAPTOP_20V.csv",
    ]);
    expect(result.records).toEqual([]);
    await settle(driver.disconnect());
  });

  it("AC-CJ3QCB.2: a refused sign-in on a signed-in session refuses the delete untouched", async () => {
    const { rec, driver } = await start();
    await settle(driver.listRecords());
    rec.spoilAuth = true;
    const from = rec.log.length;
    const { saved, save } = sink(rec);
    expect(await rejection(driver.deleteRecords(1, "PHONE_5V", save))).toEqual(SIGN_IN);
    expect(noPolls(rec.names(from))).toEqual(["auth"]);
    expect(saved).toEqual([]);
    rec.spoilAuth = false;
    expect((await settle(driver.listRecords())).records.map((r) => r.name)).toEqual(DEMO);
    await settle(driver.disconnect());
  });
});
