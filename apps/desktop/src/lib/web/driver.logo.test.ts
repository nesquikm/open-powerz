/**
 * FR W8SHXB: the startup logo over the browser session driver.
 *
 * - AC-W8SHXB.2: upload and Remove refuse before any write when SETTINGS was not read
 *   in this session, the sign-in failed, or the meter is not a KM003C, with the desktop
 *   app's reasons; the device info is read again before a write (as `prepare_write`).
 * - AC-W8SHXB.3: an upload sends only 0x4A chunks inside 0x300000–0x31D4FF (each the
 *   core's frame, 1000 ms each), then one startup-duration write of 2, then reads the
 *   area back; Remove sends one 16-byte 0xFF chunk, the value 0 and a 16-byte read. A
 *   failed chunk stops the write (no flag, no retry); a read-back difference fails.
 * - AC-W8SHXB.4: progress reports `chunks` then `read_back` steps, at least 50 ms apart
 *   as the desktop's `ProgressGate`; the stream and other commands wait meanwhile.
 * - AC-W8SHXB.5: reading the logo (head, or the whole picture) sends only memory reads
 *   inside the logo window, at least 50 ms apart.
 * - AC-W8SHXB.6: the settings check after a write reports saved, needs a menu save or
 *   calibration changed from SETTINGS reads only; with no write waiting it refuses.
 * - AC-W8SHXB.7: a HID session refuses every logo command with the basic-readings
 *   wording and sends nothing.
 *
 * Runs against the wasm demo meter on Vitest's fake clock.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { LogoProgress, LogoRead, LogoSettingsCheck, LogoWritten, UiError } from "../backend";
import { DemoTransport } from "./demoTransport";
import { MeterDriver } from "./driver";
import type { PowerzWasm } from "./driver";
import {
  LOGO_REGION,
  LOGO_WINDOW,
  bmp,
  classify,
  loadWasm,
  logoChunkOf,
  logoWasm,
  memoryReadOf,
  type FrameName,
} from "./testWasm.test";
import { timeoutError, type WebTransport } from "./transport";

type DriverOptions = Parameters<typeof MeterDriver.connect>[0];

/** What a logo write takes: a picture's bytes, or a built-in logo. */
type LogoSource = { bytes: Uint8Array; isBin: boolean; alpha: boolean } | { builtin: string };

/** The driver with the FR W8SHXB commands. */
type LogoDriver = MeterDriver & {
  logoUpload(source: LogoSource, onProgress: (p: LogoProgress) => void): Promise<LogoWritten>;
  logoRemove(onProgress: (p: LogoProgress) => void): Promise<LogoWritten>;
  logoRead(full: boolean): Promise<LogoRead>;
  logoSettingsCheck(): Promise<LogoSettingsCheck>;
};

interface Entry {
  frame: Uint8Array;
  reply: Uint8Array | null;
  name: FrameName;
  at: number;
  timeoutMs: number;
}

/** Records every frame with its reply, time and timeout; can answer or rewrite frames. */
class Recorder implements WebTransport {
  log: Entry[] = [];
  spoilAuth = false;
  /** Answers a frame without the demo meter (`null`: pass it on); `"hang"` times out. */
  swallow: ((frame: Uint8Array, name: FrameName) => Uint8Array | "hang" | null) | null = null;
  /** Rewrites a reply (`null` keeps it). */
  rewrite: ((frame: Uint8Array, reply: Uint8Array, name: FrameName) => Uint8Array | null) | null =
    null;
  constructor(
    private w: PowerzWasm,
    private inner: WebTransport,
  ) {}
  names(from = 0): FrameName[] {
    return this.log.slice(from).map((e) => e.name);
  }
  async transact(frame: Uint8Array, timeoutMs: number): Promise<Uint8Array> {
    const name = classify(this.w, frame);
    const entry: Entry = { frame: frame.slice(), reply: null, name, at: Date.now(), timeoutMs };
    this.log.push(entry);
    const swallowed = this.swallow?.(frame, name) ?? null;
    if (swallowed === "hang") {
      await new Promise((r) => setTimeout(r, timeoutMs));
      throw timeoutError();
    }
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

async function settle<T>(p: Promise<T>, maxMs = 30_000): Promise<T> {
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
  driver: LogoDriver;
  errors: UiError[];
}

async function start(
  o: { hid?: boolean; spoilAuth?: boolean; wasm?: PowerzWasm; settings?: boolean } = {},
): Promise<Run> {
  const wasm = o.wasm ?? w;
  const rec = new Recorder(w, new DemoTransport(wasm, "load-5v-1a", () => Date.now()));
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
  const driver = (await settle(MeterDriver.connect(options))) as LogoDriver;
  await vi.advanceTimersByTimeAsync(250);
  // The logo view shows the settings first, as on the desktop.
  if (o.settings !== false && o.hid !== true) {
    await settle(driver.readSettings());
  }
  return { rec, driver, errors };
}

const POLLS: readonly FrameName[] = ["get_data:adc", "get_data:adc_queue"];
const noPolls = (names: FrameName[]) => names.filter((n) => !POLLS.includes(n));
const isLogo = (n: FrameName) =>
  n.startsWith("logo_chunk:") || n.startsWith("logo_flag:") || n.startsWith("logo_read:");
/** The logo write's own frames (no polls, no device info, no sign-in). */
const logoFrames = (names: FrameName[]) =>
  names.filter((n) => isLogo(n) || n === "get_data:settings");

const BASE = LOGO_REGION[0];
const WINDOW = LOGO_WINDOW[0];
const PICTURE = bmp(100, 60);
const UPLOAD: LogoSource = { bytes: PICTURE, isBin: false, alpha: false };
const REFUSED_HINT = "Replug the meter so the app reads it again, then retry.";
const CHUNK_HINT =
  "The startup duration was not changed, so the meter keeps its old screen. Try again.";
const UPLOAD_HINT = "Try the upload again; nothing was retried.";
const SIGN_IN: UiError = {
  kind: "protocol",
  message: "The meter did not accept the app's sign-in.",
  hint: "Replug the meter and try again.",
};
const DURATIONS = ["off", "1 s", "3 s", "5 s"];

/** Progress events with the time each arrived. */
function progressSink() {
  const events: (LogoProgress & { at: number })[] = [];
  return { events, on: (p: LogoProgress) => events.push({ ...p, at: Date.now() }) };
}

const chunkEntries = (rec: Recorder, from = 0) =>
  rec.log.slice(from).filter((e) => e.name.startsWith("logo_chunk:"));
const readEntries = (rec: Recorder, from = 0) =>
  rec.log.slice(from).filter((e) => e.name.startsWith("logo_read:"));

function assertNoWrite(rec: Recorder, from: number): void {
  const written = rec.log
    .slice(from)
    .filter((e) => [0x4a, 0x48].includes((e.frame[0] ?? 0) & 0x7f) || isLogo(e.name));
  expect(written.map((e) => e.name)).toEqual([]);
}

describe("upload (AC-W8SHXB.3, .4)", () => {
  it("writes the chunks, then the value 2, reads it all back and reads SETTINGS once", async () => {
    const { rec, driver, errors } = await start();
    const preview = logoWasm(w).logoPreview(PICTURE, false, false);
    expect(preview.chunks).toBe(47);
    const from = rec.log.length;
    const { on } = progressSink();
    const result = await settle(driver.logoUpload(UPLOAD, on));

    const names = logoFrames(rec.names(from));
    const chunks = Array.from({ length: preview.chunks }, (_, k) => `logo_chunk:${BASE + k * 256}`);
    const reads = Array.from(
      { length: Math.ceil(preview.padded_size / 256) },
      (_, k) => `logo_read:${WINDOW + k * 256}`,
    );
    expect(names).toEqual([...chunks, "logo_flag:2", ...reads, "get_data:settings"]);

    // Each chunk carries the picture's padded blob, 256 bytes at its address.
    for (const [k, e] of chunkEntries(rec, from).entries()) {
      const c = logoChunkOf(e.frame);
      expect(c?.addr).toBe(BASE + k * 256);
      expect(c?.len).toBe(256);
      expect(e.timeoutMs, `chunk ${k}`).toBe(1000);
    }
    // The read-back covers the padded blob, ≤ 256 bytes a read.
    const lens = readEntries(rec, from).map((e) => memoryReadOf(e.frame)?.len ?? 0);
    expect(lens.reduce((a, b) => a + b, 0)).toBe(preview.padded_size);
    expect(Math.max(...lens)).toBeLessThanOrEqual(256);

    expect(Object.keys(result).sort()).toEqual([
      "duration_before",
      "duration_set",
      "settings",
      "startup_duration",
    ]);
    expect(DURATIONS).toContain(result.duration_before);
    expect(result.duration_set).toBe("3 s");
    // The demo meter, like the KM003C, waits for a save in its menu after the write.
    expect(result.settings).toBe("needs_menu_save");
    expect(result.startup_duration).toBeNull();
    expect(errors).toEqual([]);
    await settle(driver.disconnect());
  });

  it("the picture reads back as the preview's pixels", async () => {
    const { driver } = await start();
    const preview = logoWasm(w).logoPreview(PICTURE, false, false);
    await settle(driver.logoUpload(UPLOAD, () => {}));
    const read = await settle(driver.logoRead(true));
    expect(read).toEqual({
      status: "logo",
      width: preview.width,
      height: preview.height,
      mode: "opaque",
      image: preview.rgba,
    });
    await settle(driver.disconnect());
  });

  it("a transparent picture and a built-in logo upload as previewed", async () => {
    const { driver, rec } = await start();
    const lw = logoWasm(w);
    const alpha = lw.logoPreview(bmp(64, 48), false, true);
    await settle(driver.logoUpload({ bytes: bmp(64, 48), isBin: false, alpha: true }, () => {}));
    expect(await settle(driver.logoRead(true))).toMatchObject({ mode: "alpha", image: alpha.rgba });

    for (const name of ["surf", "surf-wordmark"]) {
      const preview = lw.logoPreviewBuiltin(name);
      const from = rec.log.length;
      const result = await settle(driver.logoUpload({ builtin: name }, () => {}));
      expect(result.duration_set).toBe("3 s");
      expect(chunkEntries(rec, from)).toHaveLength(preview.chunks);
      expect(await settle(driver.logoRead(true))).toEqual({
        status: "logo",
        width: preview.width,
        height: preview.height,
        mode: "alpha",
        image: preview.rgba,
      });
    }
    await settle(driver.disconnect());
  });

  it("reports chunks then read-back progress, at most one event per 50 ms", async () => {
    const { driver, rec } = await start();
    const from = rec.log.length;
    const { events, on } = progressSink();
    await settle(driver.logoUpload(UPLOAD, on));
    const reads = readEntries(rec, from).length;
    expect(events[0]).toMatchObject({ step: "chunks", done: 1, total: 47 });
    for (const [i, e] of events.entries()) {
      expect(["chunks", "read_back"]).toContain(e.step);
      expect(e.done).toBeGreaterThan(0);
      expect(e.done).toBeLessThanOrEqual(e.total);
      expect(e.total).toBe(e.step === "chunks" ? 47 : reads);
      const prev = events[i - 1];
      if (prev) expect(e.at - prev.at, `event ${i}`).toBeGreaterThanOrEqual(50);
      if (prev?.step === "read_back") expect(e.step).toBe("read_back");
      if (prev && prev.step === e.step) expect(e.done).toBeGreaterThan(prev.done);
    }
    const readBack = events.filter((e) => e.step === "read_back");
    expect(readBack.length).toBeGreaterThan(1);
    await settle(driver.disconnect());
  });

  it("paces the read-back reads at least 50 ms apart", async () => {
    const { driver, rec } = await start();
    const from = rec.log.length;
    await settle(driver.logoUpload(UPLOAD, () => {}));
    const reads = readEntries(rec, from);
    expect(reads.length).toBeGreaterThan(1);
    for (let i = 1; i < reads.length; i++) {
      const gap = (reads[i]?.at ?? 0) - (reads[i - 1]?.at ?? 0);
      expect(gap, `read ${i}`).toBeGreaterThanOrEqual(50);
    }
    await settle(driver.disconnect());
  });

  it("re-reads the device info before writing", async () => {
    const { driver, rec } = await start();
    const from = rec.log.length;
    await settle(driver.logoUpload(UPLOAD, () => {}));
    const names = rec.names(from);
    const firstChunk = names.findIndex((n) => n.startsWith("logo_chunk:"));
    expect(names.slice(0, firstChunk)).toContain(`memory_read:${0x0000_0420}`);
    await settle(driver.disconnect());
  });

  it("holds the stream and other commands while the write runs (AC-W8SHXB.4)", async () => {
    const { driver, rec } = await start();
    const from = rec.log.length;
    const upload = driver.logoUpload(UPLOAD, () => {});
    // A logo head read asked for meanwhile (a command that stays valid after the
    // write) waits for the write to end. Settled as soon as it exists, so it can
    // never become an unhandled rejection.
    const held = settle(driver.logoRead(false));
    const written = await settle(upload);
    const head = await held;
    const names = rec.names(from);
    const first = names.findIndex((n) => n.startsWith("logo_chunk:"));
    const writeEnd = names.indexOf("get_data:settings", first);
    expect(first).toBeGreaterThanOrEqual(0);
    expect(names.slice(first, writeEnd).filter((n) => n.startsWith("logo_read:"))).toHaveLength(
      Math.ceil(logoWasm(w).logoPreview(PICTURE, false, false).padded_size / 256),
    );
    expect(names.slice(first, writeEnd + 1).filter((n) => POLLS.includes(n))).toEqual([]);
    // Nothing but the write's own frames between its first chunk and its SETTINGS read.
    expect(names.slice(first, writeEnd).filter((n) => !isLogo(n))).toEqual([]);
    // The held read runs after the write's SETTINGS read, and sees the new logo.
    expect(names.indexOf(`logo_read:${WINDOW}`, writeEnd + 1)).toBeGreaterThan(writeEnd);
    expect(written.duration_set).toBe("3 s");
    expect(head).toMatchObject({ status: "logo", width: 100, height: 60, mode: "opaque" });
    // And the stream resumes afterwards.
    await vi.advanceTimersByTimeAsync(500);
    expect(rec.names(from + writeEnd + 1).some((n) => POLLS.includes(n))).toBe(true);
    await settle(driver.disconnect());
  });
});

describe("remove (AC-W8SHXB.3)", () => {
  it("writes one 16-byte 0xFF chunk, the value 0 and reads 16 bytes back", async () => {
    const { driver, rec } = await start();
    await settle(driver.logoUpload(UPLOAD, () => {}));
    const from = rec.log.length;
    const { events, on } = progressSink();
    const result = await settle(driver.logoRemove(on));
    expect(logoFrames(rec.names(from))).toEqual([
      `logo_chunk:${BASE}`,
      "logo_flag:0",
      `logo_read:${WINDOW}`,
      "get_data:settings",
    ]);
    const [chunk] = chunkEntries(rec, from);
    const c = chunk ? logoChunkOf(chunk.frame) : null;
    expect(c?.len).toBe(16);
    expect(Array.from(c?.data ?? [])).toEqual(Array<number>(16).fill(0xff));
    const [read] = readEntries(rec, from);
    expect(read ? memoryReadOf(read.frame) : null).toEqual({ addr: WINDOW, len: 16 });
    expect(result.duration_set).toBe("off");
    expect(DURATIONS).toContain(result.duration_before);
    expect(events[0]).toMatchObject({ step: "chunks", done: 1, total: 1 });
    expect(await settle(driver.logoRead(false))).toEqual({
      status: "empty",
      width: null,
      height: null,
      mode: null,
      image: null,
    });
    await settle(driver.disconnect());
  });
});

describe("only the logo paths (AC-W8SHXB.3)", () => {
  it("every chunk lies in the region, every SET_DATA is the duration 2 or 0, every read in the window", async () => {
    const { driver, rec } = await start();
    const from = rec.log.length;
    await settle(driver.logoUpload(UPLOAD, () => {}));
    await settle(driver.logoUpload({ builtin: "surf" }, () => {}));
    await settle(driver.logoRemove(() => {}));
    await settle(driver.logoRead(true));
    const entries = rec.log.slice(from);
    expect(entries.map((e) => e.name)).not.toContain("unknown");
    for (const e of entries) {
      const ty = (e.frame[0] ?? 0) & 0x7f;
      if (ty === 0x4a) {
        const c = logoChunkOf(e.frame);
        expect(c).not.toBeNull();
        if (!c) continue;
        expect(c.addr).toBeGreaterThanOrEqual(LOGO_REGION[0]);
        expect(c.addr + c.len).toBeLessThanOrEqual(LOGO_REGION[1]);
      }
      if (ty === 0x48) expect(["logo_flag:2", "logo_flag:0"]).toContain(e.name);
    }
    for (const e of readEntries(rec, from)) {
      const r = memoryReadOf(e.frame);
      expect(r).not.toBeNull();
      if (!r) continue;
      expect(r.addr).toBeGreaterThanOrEqual(LOGO_WINDOW[0]);
      expect(r.addr + r.len).toBeLessThanOrEqual(LOGO_WINDOW[1]);
    }
    await settle(driver.disconnect());
  });
});

describe("failures stop the write, nothing is retried (AC-W8SHXB.3)", () => {
  it("a rejected chunk stops the upload: no further chunk, no flag, no read-back", async () => {
    const { driver, rec } = await start();
    let seen = 0;
    rec.swallow = (frame, name) => {
      if (!name.startsWith("logo_chunk:")) return null;
      return seen++ === 2 ? new Uint8Array([0x06, frame[1] ?? 0, 0, 0]) : null;
    };
    const from = rec.log.length;
    const error = await rejection(driver.logoUpload(UPLOAD, () => {}));
    expect(error).toEqual({
      kind: "protocol",
      message: "Writing the logo stopped at chunk 2: REJECT.",
      hint: CHUNK_HINT,
    });
    expect(logoFrames(rec.names(from))).toEqual([
      `logo_chunk:${BASE}`,
      `logo_chunk:${BASE + 256}`,
      `logo_chunk:${BASE + 512}`,
    ]);
    await settle(driver.disconnect());
  });

  it("a chunk with no reply stops after its 1000 ms, with no retry", async () => {
    const { driver, rec } = await start();
    rec.swallow = (_frame, name) => (name.startsWith("logo_chunk:") ? "hang" : null);
    const from = rec.log.length;
    const t0 = Date.now();
    const error = await rejection(driver.logoUpload(UPLOAD, () => {}));
    expect(error.kind).toBe("protocol");
    expect(error.message.startsWith("Writing the logo stopped at chunk 0: ")).toBe(true);
    expect(error.hint).toBe(CHUNK_HINT);
    expect(chunkEntries(rec, from)).toHaveLength(1);
    expect(chunkEntries(rec, from)[0]?.timeoutMs).toBe(1000);
    expect(Date.now() - t0).toBeGreaterThanOrEqual(1000);
    expect(logoFrames(rec.names(from))).toEqual([`logo_chunk:${BASE}`]);
    await settle(driver.disconnect());
  });

  it("a rejected flag write fails with the desktop wording and reads nothing back", async () => {
    const { driver, rec } = await start();
    rec.swallow = (frame, name) =>
      name === "logo_flag:2" ? new Uint8Array([0x06, frame[1] ?? 0, 0, 0]) : null;
    const from = rec.log.length;
    const error = await rejection(driver.logoUpload(UPLOAD, () => {}));
    expect(error).toEqual({
      kind: "protocol",
      message: "The logo was written but its flag write failed: REJECT.",
      hint: UPLOAD_HINT,
    });
    expect(readEntries(rec, from)).toEqual([]);
    expect(rec.names(from).filter((n) => n === "logo_flag:2")).toHaveLength(1);
    await settle(driver.disconnect());
  });

  it("a read-back that differs fails naming the offset", async () => {
    const { driver, rec } = await start();
    // Chunk 1 is answered FINISHED but never reaches the flash.
    rec.swallow = (frame, name) =>
      name === `logo_chunk:${BASE + 256}` ? new Uint8Array([0x07, frame[1] ?? 0, 0, 0]) : null;
    const from = rec.log.length;
    const error = await rejection(driver.logoUpload(UPLOAD, () => {}));
    expect(error.kind).toBe("protocol");
    expect(error.message).toMatch(
      /^The logo read back from the meter differs from the one written, at offset 0x1[0-9a-f]{2}\.$/,
    );
    expect(error.hint).toBe(UPLOAD_HINT);
    expect(chunkEntries(rec, from)).toHaveLength(47);
    expect(rec.names(from).filter((n) => n === "get_data:settings")).toEqual([]);
    await settle(driver.disconnect());
  });

  it("a Remove whose area is not erased fails at the read-back step", async () => {
    const { driver, rec } = await start();
    await settle(driver.logoUpload(UPLOAD, () => {}));
    rec.swallow = (frame, name) =>
      name.startsWith("logo_chunk:") ? new Uint8Array([0x07, frame[1] ?? 0, 0, 0]) : null;
    const error = await rejection(driver.logoRemove(() => {}));
    expect(error.kind).toBe("protocol");
    expect(
      error.message.startsWith(
        "Removing the logo failed at the read-back step: expected 16 bytes of 0xFF, read [",
      ),
    ).toBe(true);
    expect(error.hint).toBe("Try Remove again; nothing was retried.");
    await settle(driver.disconnect());
  });
});

describe("refused before any write (AC-W8SHXB.2)", () => {
  it("without a SETTINGS read in this session", async () => {
    const { driver, rec } = await start({ settings: false });
    for (const call of [
      () => driver.logoUpload(UPLOAD, () => {}),
      () => driver.logoUpload({ builtin: "surf" }, () => {}),
      () => driver.logoRemove(() => {}),
    ]) {
      const from = rec.log.length;
      expect(await rejection(call())).toEqual({
        kind: "usage",
        message: "The logo was not written: SETTINGS was not read in this session.",
        hint: REFUSED_HINT,
      });
      assertNoWrite(rec, from);
    }
    await settle(driver.disconnect());
  });

  it("when the sign-in fails", async () => {
    const { driver, rec } = await start({ spoilAuth: true });
    expect(driver.authLevel).toBe(0);
    for (const call of [
      () => driver.logoUpload(UPLOAD, () => {}),
      () => driver.logoRemove(() => {}),
    ]) {
      const from = rec.log.length;
      expect(await rejection(call())).toEqual(SIGN_IN);
      assertNoWrite(rec, from);
    }
    await settle(driver.disconnect());
  });

  it("when the meter is not a KM003C", async () => {
    // The device info decodes as a KM002C.
    const km002c = {
      ...w,
      decodeDeviceInfo: (...a: Parameters<PowerzWasm["decodeDeviceInfo"]>) => ({
        ...w.decodeDeviceInfo(...a),
        model: "KM002C",
      }),
    } as unknown as PowerzWasm;
    const { driver, rec } = await start({ wasm: km002c });
    for (const call of [
      () => driver.logoUpload(UPLOAD, () => {}),
      () => driver.logoRemove(() => {}),
    ]) {
      const from = rec.log.length;
      expect(await rejection(call())).toEqual({
        kind: "usage",
        message: "The logo was not written: the meter is a KM002C, logo upload needs a KM003C.",
        hint: REFUSED_HINT,
      });
      assertNoWrite(rec, from);
    }
    await settle(driver.disconnect());
  });

  it("a picture that is no picture is a usage error and sends nothing", async () => {
    const { driver, rec } = await start();
    const from = rec.log.length;
    const error = await rejection(
      driver.logoUpload(
        { bytes: new TextEncoder().encode("nope"), isBin: false, alpha: false },
        () => {},
      ),
    );
    expect(error.kind).toBe("usage");
    assertNoWrite(rec, from);
    await settle(driver.disconnect());
  });
});

describe("reading the logo (AC-W8SHXB.5)", () => {
  it("a head read of an erased area is one 16-byte read in the window", async () => {
    const { driver, rec } = await start();
    const from = rec.log.length;
    const read = await settle(driver.logoRead(false));
    expect(read).toEqual({ status: "empty", width: null, height: null, mode: null, image: null });
    const names = noPolls(rec.names(from)).filter((n) => n !== "auth");
    expect(names).toEqual([`logo_read:${WINDOW}`]);
    const [e] = readEntries(rec, from);
    expect(e ? memoryReadOf(e.frame) : null).toEqual({ addr: WINDOW, len: 16 });
    await settle(driver.disconnect());
  });

  it("a full read sends only window reads, at least 50 ms apart", async () => {
    const { driver, rec } = await start();
    await settle(driver.logoUpload(UPLOAD, () => {}));
    const from = rec.log.length;
    const read = await settle(driver.logoRead(true));
    expect(read.status).toBe("logo");
    expect(read.image).toBe(logoWasm(w).logoPreview(PICTURE, false, false).rgba);
    const names = noPolls(rec.names(from)).filter((n) => n !== "auth");
    expect(names.length).toBeGreaterThan(2);
    for (const n of names) expect(n.startsWith("logo_read:"), n).toBe(true);
    const reads = readEntries(rec, from);
    for (let i = 1; i < reads.length; i++) {
      expect((reads[i]?.at ?? 0) - (reads[i - 1]?.at ?? 0), `read ${i}`).toBeGreaterThanOrEqual(50);
    }
    expect(rec.log.slice(from).some((e) => [0x4a, 0x48].includes((e.frame[0] ?? 0) & 0x7f))).toBe(
      false,
    );
    await settle(driver.disconnect());
  });

  it("needs no SETTINGS read", async () => {
    const { driver } = await start({ settings: false });
    expect((await settle(driver.logoRead(false))).status).toBe("empty");
    await settle(driver.disconnect());
  });
});

/** CRC-32 (IEEE), for re-sealing a test SETTINGS block. */
function crc32(data: Uint8Array): number {
  let c = 0xffff_ffff;
  for (const b of data) {
    c ^= b;
    for (let k = 0; k < 8; k++) c = c & 1 ? (c >>> 1) ^ 0xedb8_8320 : c >>> 1;
  }
  return (c ^ 0xffff_ffff) >>> 0;
}

/** The SETTINGS reply `reply` with calibration byte `at` (0x1C..0x5C) changed and block
 * A's CRC made valid again. */
function withOtherCalibration(reply: Uint8Array, at: number): Uint8Array {
  const out = reply.slice();
  const payload = out.subarray(8);
  payload[at] = (payload[at] ?? 0) ^ 0x10;
  new DataView(payload.buffer, payload.byteOffset).setUint32(
    0x5c,
    crc32(payload.subarray(0, 0x5c)),
    true,
  );
  return out;
}

/** `reply` carrying `frame`'s request id. */
function answering(frame: Uint8Array, reply: Uint8Array): Uint8Array {
  const out = reply.slice();
  out[1] = frame[1] ?? 0;
  return out;
}

const settingsReplies = (rec: Recorder, from = 0) =>
  rec.log
    .slice(from)
    .filter((e) => e.name === "get_data:settings")
    .map((e) => e.reply as Uint8Array);

describe("the settings check (AC-W8SHXB.6)", () => {
  it("with no write waiting it refuses and sends nothing", async () => {
    const { driver, rec } = await start();
    const from = rec.log.length;
    expect(await rejection(driver.logoSettingsCheck())).toMatchObject({
      kind: "usage",
      message: "No logo write is waiting for a settings save.",
    });
    expect(noPolls(rec.names(from))).toEqual([]);
    await settle(driver.disconnect());
  });

  it("waits for the menu save, then reports saved from SETTINGS reads only", async () => {
    const { driver, rec } = await start();
    const [before] = settingsReplies(rec);
    if (!before) throw new Error("no SETTINGS read before the write");
    const written = await settle(driver.logoUpload(UPLOAD, () => {}));
    expect(written.settings).toBe("needs_menu_save");

    let from = rec.log.length;
    expect(await settle(driver.logoSettingsCheck())).toEqual({
      settings: "needs_menu_save",
      startup_duration: null,
    });
    expect(noPolls(rec.names(from))).toEqual(["get_data:settings"]);

    // The meter saved in its menu: its block is valid again, calibration unchanged.
    rec.rewrite = (f, _r, name) => (name === "get_data:settings" ? answering(f, before) : null);
    from = rec.log.length;
    const saved = await settle(driver.logoSettingsCheck());
    expect(saved.settings).toBe("saved");
    expect(DURATIONS).toContain(saved.startup_duration);
    expect(noPolls(rec.names(from))).toEqual(["get_data:settings"]);

    // Saved ends the wait.
    from = rec.log.length;
    expect(await rejection(driver.logoSettingsCheck())).toMatchObject({
      kind: "usage",
      message: "No logo write is waiting for a settings save.",
    });
    expect(noPolls(rec.names(from))).toEqual([]);
    await settle(driver.disconnect());
  });

  it("reports calibration changed and ends the wait", async () => {
    const { driver, rec } = await start();
    const [before] = settingsReplies(rec);
    if (!before) throw new Error("no SETTINGS read before the write");
    // An upload writes the value 2, which leaves block A's CRC stale on the demo
    // meter (as on the KM003C), so the write waits for a menu save.
    const written = await settle(driver.logoUpload(UPLOAD, () => {}));
    expect(written.settings).toBe("needs_menu_save");
    rec.rewrite = (f, _r, name) =>
      name === "get_data:settings" ? answering(f, withOtherCalibration(before, 0x1c + 3)) : null;
    expect(await settle(driver.logoSettingsCheck())).toEqual({
      settings: "calibration_changed",
      startup_duration: null,
    });
    expect(await rejection(driver.logoSettingsCheck())).toMatchObject({ kind: "usage" });
    await settle(driver.disconnect());
  });

  it("a write whose fresh read finds the meter saved leaves nothing waiting", async () => {
    const { driver, rec } = await start();
    const [before] = settingsReplies(rec);
    if (!before) throw new Error("no SETTINGS read before the write");
    // Every SETTINGS read after the write shows a saved block.
    rec.rewrite = (f, _r, name) => (name === "get_data:settings" ? answering(f, before) : null);
    const written = await settle(driver.logoUpload(UPLOAD, () => {}));
    expect(written.settings).toBe("saved");
    expect(DURATIONS).toContain(written.startup_duration);
    expect(await rejection(driver.logoSettingsCheck())).toMatchObject({
      kind: "usage",
      message: "No logo write is waiting for a settings save.",
    });
    await settle(driver.disconnect());
  });
});

describe("over HID (AC-W8SHXB.7)", () => {
  it("every logo command refuses with the basic-readings wording and sends nothing", async () => {
    const { driver, rec } = await start({ hid: true });
    const from = rec.log.length;
    for (const call of [
      () => driver.logoUpload(UPLOAD, () => {}),
      () => driver.logoUpload({ builtin: "surf" }, () => {}),
      () => driver.logoRemove(() => {}),
      () => driver.logoRead(false),
      () => driver.logoRead(true),
      () => driver.logoSettingsCheck(),
    ]) {
      expect(await rejection(call())).toMatchObject({
        kind: "usage",
        message: "The startup logo is not available over HID: it gives basic readings only.",
      });
    }
    expect(rec.names(from).filter((n) => n !== "get_data:adc")).toEqual([]);
    await settle(driver.disconnect());
  });
});
