/**
 * FR QFTCWZ: the built wasm module the web tests run against, plus the frame classifier
 * the closed-command-set test (AC-QFTCWZ.5) and the driver tests use.
 *
 * This file is itself a test file (Vitest picks it up) so it can live next to the
 * other `*.test.ts` files without being shipped; it exports helpers and checks that
 * the classifier knows the builders.
 */

import { existsSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import type { PowerzWasm } from "./driver";

const GLUE_URL = new URL("../../wasm/powerz_wasm.js", import.meta.url);
const MODULE_URL = new URL("../../wasm/powerz_wasm_bg.wasm", import.meta.url);

let cached: PowerzWasm | null = null;

/** The generated glue, initialised once per test file. */
export async function loadWasm(): Promise<PowerzWasm> {
  if (cached) return cached;
  const glue = fileURLToPath(GLUE_URL);
  if (!existsSync(glue)) throw new Error(`${glue} is missing: run bun run build:wasm first`);
  const mod = (await import(/* @vite-ignore */ GLUE_URL.href)) as {
    initSync(o: { module: Uint8Array }): unknown;
  };
  mod.initSync({ module: readFileSync(fileURLToPath(MODULE_URL)) });
  cached = mod as unknown as PowerzWasm;
  return cached;
}

/** The four memory blocks on the safe list (hardware, firmware, OTP, UID). */
export const SAFE_BLOCKS: readonly (readonly [number, number])[] = [
  [0x0000_0420, 64],
  [0x0000_4420, 64],
  [0x0300_0c00, 64],
  [0x4001_0450, 12],
];

/** GET_DATA attribute masks a read-only session may ask for. */
export const READ_MASKS = {
  adc: 0x001,
  adc_queue: 0x002,
  settings: 0x008,
  pd_packet: 0x010,
  pd_status: 0x020,
  record_list: 0x200,
} as const;

/** The FR W8SHX8 wasm exports the tests use (settings and offline records). */
export interface RecordWasm {
  settingsRequest(id: number): Uint8Array;
  decodeSettings(reply: Uint8Array): { fields: unknown[]; warnings: string[] };
  RecordBook: new () => RecordBookLike;
}

export interface RecordBookLike {
  listRequest(id: number): Uint8Array;
  decodeList(reply: Uint8Array): { records: { index: number; name: string; points: number }[] };
  chunks(index: number): { addr: number; len: number }[];
  chunkRequest(id: number, addr: number, len: number): Uint8Array;
  decodeRecord(index: number, data: Uint8Array): unknown;
  free(): void;
}

/** The FR W8SHXA delete methods of the wasm `RecordBook`. */
export interface RecordDeleteBookLike extends RecordBookLike {
  /** 1-based targets over the latest list (`undefined` = every record), or a throw. */
  deleteTargets(index: number | undefined, confirm: string): Uint32Array | number[];
  /** The record's desktop backup file from its window's bytes. */
  backupFile(index: number, data: Uint8Array): { name: string; csv: string };
  /** The SET_DATA 0x48 att 0x28 frame that keeps every listed record but `indices`. */
  deleteRequest(id: number, indices: Uint32Array): Uint8Array;
  takeDeleteReply(reply: Uint8Array): void;
  checkAfterDelete(): void;
}

/** The FR W8SHX8 exports of `w`. */
export function recordWasm(w: PowerzWasm): RecordWasm {
  return w as unknown as RecordWasm;
}

/** The demo meter's RecordList reply, read after CONNECT and a level-1 auth. */
export function demoRecordListReply(w: PowerzWasm): Uint8Array {
  const rw = recordWasm(w);
  const m = new w.DemoMeter("idle");
  try {
    w.decodeAccept(m.transact(w.connectRequest(1)));
    const [addr, len] = SAFE_BLOCKS[3] as readonly [number, number];
    const uid = w.decodeMemoryRead(m.transact(w.memoryReadRequest(2, addr, len)), addr, len);
    const now = 1_790_000_000_000n;
    const random = 0x0123_4567_89ab_cdefn;
    const level = w.decodeAuthReply(
      m.transact(w.authRequest(3, 1, uid, now, random)),
      uid,
      now,
      random,
    );
    if (level < 1) throw new Error("the demo meter refused the level-1 auth");
    const book = new rw.RecordBook();
    try {
      return m.transact(book.listRequest(4));
    } finally {
      book.free();
    }
  } finally {
    (m as unknown as { free(): void }).free();
  }
}

/** The demo meter's record reads: each listed record's chunks, by 1-based index. */
let demoChunks: { w: PowerzWasm; book: RecordBookLike; chunks: [number, number, number][] } | null =
  null;

function recordChunks(w: PowerzWasm): typeof demoChunks {
  if (demoChunks?.w === w) return demoChunks;
  const rw = recordWasm(w);
  if (typeof rw.RecordBook !== "function") return null;
  const book = new rw.RecordBook();
  const list = book.decodeList(demoRecordListReply(w));
  const chunks: [number, number, number][] = [];
  for (const r of list.records) {
    if (r.points === 0) continue;
    for (const c of book.chunks(r.index)) chunks.push([r.index, c.addr, c.len]);
  }
  demoChunks = { w, book, chunks };
  return demoChunks;
}

export type FrameName =
  | "connect"
  | "auth"
  | `get_data:${keyof typeof READ_MASKS}`
  | `start_queue:${number}`
  | "stop_queue"
  | "start_pd"
  | "stop_pd"
  | "pd_poll"
  | "pd_poll_adc"
  | `memory_read:${number}`
  | `record_read:${number}`
  | "record_table_write"
  | "unknown";

function same(a: Uint8Array, b: Uint8Array): boolean {
  return a.length === b.length && a.every((v, i) => v === b[i]);
}

/** Names `frame` by rebuilding every safe request with the frame's id byte. */
export function classify(w: PowerzWasm, frame: Uint8Array): FrameName {
  const id = frame[1] ?? 0;
  if (same(frame, w.connectRequest(id))) return "connect";
  for (const [name, mask] of Object.entries(READ_MASKS)) {
    if (same(frame, w.getDataRequest(id, mask))) return `get_data:${name}` as FrameName;
  }
  for (let code = 0; code <= 3; code++) {
    if (same(frame, w.startQueueRequest(id, code))) return `start_queue:${code}`;
  }
  if (same(frame, w.stopQueueRequest(id))) return "stop_queue";
  if (same(frame, w.startPdSnifferRequest(id))) return "start_pd";
  if (same(frame, w.stopPdSnifferRequest(id))) return "stop_pd";
  if (same(frame, w.pdPollRequest(id, false))) return "pd_poll";
  if (same(frame, w.pdPollRequest(id, true))) return "pd_poll_adc";
  for (const [addr, len] of SAFE_BLOCKS) {
    if (same(frame, w.memoryReadRequest(id, addr, len))) return `memory_read:${addr}`;
  }
  // A read of a demo record's own window (FR W8SHX8), by the record's index.
  if (frame[0] === 0x44) {
    const demo = recordChunks(w);
    for (const [index, addr, len] of demo ? demo.chunks : []) {
      if (demo && same(frame, demo.book.chunkRequest(id, addr, len))) return `record_read:${index}`;
    }
  }
  // The record-table write (FR W8SHXA): SET_DATA whose one section is attribute 0x28.
  if (((frame[0] ?? 0) & 0x7f) === 0x48 && frame.length >= 8) {
    const att = ((frame[4] ?? 0) | ((frame[5] ?? 0) << 8)) & 0x7fff;
    return att === 0x28 ? "record_table_write" : "unknown";
  }
  // Auth carries time and random, so only its type byte and length are fixed.
  const authShape = w.authRequest(0, 1, new Uint8Array(12), 0n, 0n);
  if (frame.length === authShape.length && frame[0] === authShape[0]) return "auth";
  return "unknown";
}

describe("test wasm helpers", () => {
  it("classifies each builder's output by name", async () => {
    const w = await loadWasm();
    expect(classify(w, w.connectRequest(7))).toBe("connect");
    expect(classify(w, w.getDataRequest(9, READ_MASKS.adc_queue))).toBe("get_data:adc_queue");
    expect(classify(w, w.startQueueRequest(3, 2))).toBe("start_queue:2");
    expect(classify(w, w.memoryReadRequest(4, 0x4001_0450, 12))).toBe(`memory_read:${0x4001_0450}`);
    expect(classify(w, new Uint8Array([0x48, 1, 0, 0]))).toBe("unknown");
  });

  it("classifies the record list and each demo record's chunk reads (FR W8SHX8)", async () => {
    const w = await loadWasm();
    expect(classify(w, w.getDataRequest(5, READ_MASKS.record_list))).toBe("get_data:record_list");
    const book = new (recordWasm(w).RecordBook)();
    book.decodeList(demoRecordListReply(w));
    const [first] = book.chunks(2);
    expect(first).toBeDefined();
    if (first)
      expect(classify(w, book.chunkRequest(6, first.addr, first.len))).toBe("record_read:2");
    book.free();
  });

  it("classifies the record-table write the core builds (FR W8SHXA)", async () => {
    const w = await loadWasm();
    const book = new (recordWasm(w).RecordBook)() as RecordDeleteBookLike;
    book.decodeList(demoRecordListReply(w));
    expect(classify(w, book.deleteRequest(7, new Uint32Array([1])))).toBe("record_table_write");
    expect(classify(w, book.deleteRequest(8, new Uint32Array([1, 2, 3])))).toBe(
      "record_table_write",
    );
    book.free();
    // Any other SET_DATA attribute stays unknown.
    expect(classify(w, new Uint8Array([0x48, 1, 0x80, 0x00, 0x1c, 0x00, 0x40, 0x00]))).toBe(
      "unknown",
    );
  });
});
