/**
 * FR QFTCWZ: the built wasm module the web tests run against, plus the frame classifier
 * the closed-command-set test (AC-QFTCWZ.5) and the driver tests use.
 *
 * This file is itself a test file (Vitest picks it up) so it can live next to the
 * other `*.test.ts` files without being shipped; it exports helpers and checks that
 * the classifier knows the builders.
 */

import { createCipheriv, createDecipheriv } from "node:crypto";
import { existsSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import type { LogoPreview, LogoRead, LogoSettingsCheck, LogoWritten } from "../backend";
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
  | `logo_chunk:${number}`
  | `logo_flag:${number}`
  | `logo_read:${number}`
  | "unknown";

// ---- FR W8SHXB: the startup logo frames ----

/** AES key 0 (memory reads and logo chunks), as the protocol core ships it. */
const KEY0 = new TextEncoder().encode("Lh2yfB7n6X7d9a5Z");

/** The flash region a logo chunk may write: 0x300000..0x31D500. */
export const LOGO_REGION = [0x30_0000, 0x31_d500] as const;
/** The memory window a logo read may read: 0x98300000..0x98320000. */
export const LOGO_WINDOW = [0x9830_0000, 0x9832_0000] as const;

/** AES-128-ECB under key 0 of whole blocks (test-side, independent of the core). */
export function decryptKey0(data: Uint8Array): Uint8Array {
  const d = createDecipheriv("aes-128-ecb", KEY0, null);
  d.setAutoPadding(false);
  return new Uint8Array(Buffer.concat([d.update(data), d.final()]));
}

export function encryptKey0(data: Uint8Array): Uint8Array {
  const c = createCipheriv("aes-128-ecb", KEY0, null);
  c.setAutoPadding(false);
  return new Uint8Array(Buffer.concat([c.update(data), c.final()]));
}

const u32 = (b: Uint8Array, at: number) =>
  ((b[at] ?? 0) | ((b[at + 1] ?? 0) << 8) | ((b[at + 2] ?? 0) << 16) | ((b[at + 3] ?? 0) << 24)) >>>
  0;

/** A 0x4A logo chunk decrypted: its flash address, length and data; `null` for any
 * other frame or a block header that is not a chunk's. */
export function logoChunkOf(
  frame: Uint8Array,
): { addr: number; len: number; data: Uint8Array } | null {
  if (((frame[0] ?? 0) & 0x7f) !== 0x4a || frame.length < 4 + 32) return null;
  const payload = frame.slice(4);
  if (payload.length % 16 !== 0) return null;
  const plain = decryptKey0(payload);
  const addr = u32(plain, 0);
  const len = u32(plain, 4);
  if (u32(plain, 8) !== 0xffff_ffff || 16 + len > plain.length) return null;
  return { addr, len, data: plain.slice(16, 16 + len) };
}

/** A 0x44 memory read decrypted: its address and length; `null` for any other frame. */
export function memoryReadOf(frame: Uint8Array): { addr: number; len: number } | null {
  if (frame[0] !== 0x44 || frame.length !== 4 + 32) return null;
  const plain = decryptKey0(frame.slice(4));
  if (u32(plain, 8) !== 0xffff_ffff || plain.slice(16).some((b) => b !== 0xff)) return null;
  return { addr: u32(plain, 0), len: u32(plain, 4) };
}

/** The value of a startup-duration write (SET_DATA 0x48, one section att 0x1C of 4
 * bytes); `null` for any other frame. */
export function logoFlagOf(frame: Uint8Array): number | null {
  if (((frame[0] ?? 0) & 0x7f) !== 0x48 || frame.length !== 12) return null;
  const main = u32(frame, 0);
  const ext = u32(frame, 4);
  if (main >>> 22 !== 3 || (ext & 0x3f_ffff) !== 0x1c || ext >>> 22 !== 4) return null;
  return u32(frame, 8);
}

/** The logo chunk's name: inside the region, on the 256-byte step (or the 16-byte
 * Remove clear at the start), never past the end. */
function logoChunkName(frame: Uint8Array): FrameName | null {
  const c = logoChunkOf(frame);
  if (!c) return null;
  const [start, end] = LOGO_REGION;
  const stepped = (c.addr - start) % 256 === 0 && c.len === 256;
  const clear = c.addr === start && c.len === 16 && c.data.every((b) => b === 0xff);
  const inside = c.addr >= start && c.addr + c.len <= end;
  return inside && (stepped || clear) ? `logo_chunk:${c.addr}` : "unknown";
}

function logoReadName(frame: Uint8Array): FrameName | null {
  const r = memoryReadOf(frame);
  if (!r) return null;
  const [start, end] = LOGO_WINDOW;
  if (r.addr < start || r.addr >= end) return null;
  return r.len > 0 && r.len <= 256 && r.addr + r.len <= end ? `logo_read:${r.addr}` : "unknown";
}

/** The FR W8SHXB wasm exports (startup logo). */
export interface LogoWriteLike {
  preconditions(
    authLevel: number,
    settingsReply: Uint8Array | undefined,
    model: string | undefined,
  ): void;
  chunkCount(): number;
  chunkRequest(id: number, k: number): Uint8Array;
  takeChunkReply(k: number, reply: Uint8Array): void;
  flagRequest(id: number): Uint8Array;
  takeFlagReply(reply: Uint8Array): void;
  readPlan(): { addr: number; len: number }[];
  readRequest(id: number, addr: number, len: number): Uint8Array;
  takeRead(addr: number, data: Uint8Array): void;
  settingsRequest(id: number): Uint8Array;
  takeSettings(reply: Uint8Array): LogoWritten;
  pendingSnapshot(): Uint8Array | undefined;
  free(): void;
}

export interface LogoReaderLike {
  readPlan(): { addr: number; len: number }[];
  readRequest(id: number, addr: number, len: number): Uint8Array;
  takeRead(addr: number, data: Uint8Array): void;
  result(): LogoRead;
  free(): void;
}

export interface LogoWasm {
  logoPreview(bytes: Uint8Array, isBin: boolean, alpha: boolean): LogoPreview;
  logoPreviewBuiltin(name: string): LogoPreview;
  LogoWrite: {
    upload(bytes: Uint8Array, isBin: boolean, alpha: boolean): LogoWriteLike;
    uploadBuiltin(name: string): LogoWriteLike;
    remove(): LogoWriteLike;
  };
  LogoReader: new (full: boolean) => LogoReaderLike;
  settingsSaveCheck(snapshot: Uint8Array, settingsReply: Uint8Array): LogoSettingsCheck;
}

/** The FR W8SHXB exports of `w`. */
export function logoWasm(w: PowerzWasm): LogoWasm {
  return w as unknown as LogoWasm;
}

/** The demo meter's SETTINGS reply. */
export function demoSettingsReply(w: PowerzWasm): Uint8Array {
  const m = new w.DemoMeter("idle");
  try {
    w.decodeAccept(m.transact(w.connectRequest(1)));
    return m.transact(recordWasm(w).settingsRequest(2));
  } finally {
    (m as unknown as { free(): void }).free();
  }
}

/** A 24-bit BMP of `w × h` with a gradient (the Rust tests' picture). */
export function bmp(w: number, h: number): Uint8Array {
  const row = Math.ceil((w * 3) / 4) * 4;
  const data = row * h;
  const f = new Uint8Array(54 + data);
  const v = new DataView(f.buffer);
  f[0] = 0x42;
  f[1] = 0x4d;
  v.setUint32(2, 54 + data, true);
  v.setUint32(10, 54, true);
  v.setUint32(14, 40, true);
  v.setInt32(18, w, true);
  v.setInt32(22, h, true);
  v.setUint16(26, 1, true);
  v.setUint16(28, 24, true);
  v.setUint32(34, data, true);
  v.setUint32(38, 2835, true);
  v.setUint32(42, 2835, true);
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      const at = 54 + y * row + x * 3;
      f[at] = (x * 2) & 0xff;
      f[at + 1] = (y * 4) & 0xff;
      f[at + 2] = ((x + y) * 3) & 0xff;
    }
  }
  return f;
}

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
  // A read inside the startup-logo window (FR W8SHXB), by its address.
  const logoRead = logoReadName(frame);
  if (logoRead) return logoRead;
  // A startup-logo flash chunk (FR W8SHXB), by its flash address.
  const logoChunk = logoChunkName(frame);
  if (logoChunk) return logoChunk;
  if (((frame[0] ?? 0) & 0x7f) === 0x4a) return "unknown";
  // The startup-duration write of a logo write (FR W8SHXB), by its value.
  const flag = logoFlagOf(frame);
  if (flag !== null) return flag === 0 || flag === 2 ? `logo_flag:${flag}` : "unknown";
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

describe("startup logo frames (FR W8SHXB)", () => {
  it("classifies the chunk, flag and read frames the core builds", async () => {
    const w = await loadWasm();
    const lw = logoWasm(w);
    const reply = demoSettingsReply(w);
    const rm = lw.LogoWrite.remove();
    rm.preconditions(1, reply, "KM003C");
    expect(classify(w, rm.chunkRequest(5, 0))).toBe(`logo_chunk:${0x30_0000}`);
    rm.takeChunkReply(0, new Uint8Array([0x07, 5, 0, 0]));
    expect(classify(w, rm.flagRequest(6))).toBe("logo_flag:0");
    rm.takeFlagReply(new Uint8Array([0x07, 6, 0, 0]));
    const [read] = rm.readPlan();
    if (!read) throw new Error("no read-back planned");
    expect(classify(w, rm.readRequest(7, read.addr, read.len))).toBe(`logo_read:${0x9830_0000}`);
    rm.free();

    const up = lw.LogoWrite.upload(bmp(100, 60), false, false);
    up.preconditions(1, reply, "KM003C");
    const n = up.chunkCount();
    expect(n).toBe(47);
    for (let k = 0; k < n; k++) {
      expect(classify(w, up.chunkRequest(k + 1, k))).toBe(`logo_chunk:${0x30_0000 + k * 256}`);
      up.takeChunkReply(k, new Uint8Array([0x07, k + 1, 0, 0]));
    }
    expect(classify(w, up.flagRequest(9))).toBe("logo_flag:2");
    up.free();
  });

  it("names chunks outside the region, other durations and other reads unknown", async () => {
    const w = await loadWasm();
    const chunk = (addr: number, len: number) => {
      const plain = new Uint8Array(16 + len).fill(0xff);
      new DataView(plain.buffer).setUint32(0, addr, true);
      new DataView(plain.buffer).setUint32(4, len, true);
      const main = (0x4a | (1 << 8) | (1 << 16) | ((len + 16) << 22)) >>> 0;
      const f = new Uint8Array(4 + plain.length);
      new DataView(f.buffer).setUint32(0, main, true);
      f.set(encryptKey0(plain), 4);
      return f;
    };
    expect(classify(w, chunk(0x30_0100, 256))).toBe(`logo_chunk:${0x30_0100}`);
    for (const [addr, len] of [
      [0x31_d500, 256],
      [0x2f_ff00, 256],
      [0x30_0010, 256],
      [0x30_0100, 16],
    ] as const) {
      expect(classify(w, chunk(addr, len)), addr.toString(16)).toBe("unknown");
    }
    const flag = (value: number) => {
      const f = new Uint8Array(12);
      const v = new DataView(f.buffer);
      v.setUint32(0, (0x48 | (1 << 8) | (3 << 22)) >>> 0, true);
      v.setUint32(4, (0x1c | (4 << 22)) >>> 0, true);
      v.setUint32(8, value, true);
      return f;
    };
    expect(classify(w, flag(2))).toBe("logo_flag:2");
    expect(classify(w, flag(0))).toBe("logo_flag:0");
    expect(classify(w, flag(1))).toBe("unknown");
    expect(classify(w, flag(3))).toBe("unknown");
    const read = (addr: number, len: number) => {
      const plain = new Uint8Array(32).fill(0xff);
      new DataView(plain.buffer).setUint32(0, addr, true);
      new DataView(plain.buffer).setUint32(4, len, true);
      const f = new Uint8Array(36);
      f.set([0x44, 1, 1, 1]);
      f.set(encryptKey0(plain), 4);
      return f;
    };
    expect(classify(w, read(0x9830_0000, 16))).toBe(`logo_read:${0x9830_0000}`);
    expect(classify(w, read(0x9831_ff00, 256))).toBe(`logo_read:${0x9831_ff00}`);
    expect(classify(w, read(0x9831_fff0, 32))).toBe("unknown");
    expect(classify(w, read(0x9830_0000, 257))).toBe("unknown");
  });
});
