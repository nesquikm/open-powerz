/**
 * FR X3H3JY (web parity HIL 3, BUG-8): a repeated settings change in the browser
 * failed although it landed, because the settings pipe took the meter's stream
 * frames on its serial port for replies.
 *
 * - AC-X3H3JY.1: two changes back to back on the same granted port both succeed
 *   while the port carries ADCNEW stream frames (type 0x02) between and inside the
 *   replies and a partial frame left from the earlier session; through the real
 *   `serialPipeOpener` → `openSerialLink` → `CdcPipe` → `MeterDriver.writeSettings`.
 * - AC-X3H3JY.2: stream frames, PDM text lines and noise bytes never count as
 *   another program's reply; a whole data reply for another attribute still stops
 *   the change with the in-use message before the write.
 * - AC-X3H3JY.3: a SETTINGS reply that fails its block check is read again (a new
 *   request, at most 3 reads in all for that step); the write is never sent twice;
 *   a read-back without the change still fails naming the field.
 * - AC-X3H3JY.4: opening the port this page itself still holds open closes it and
 *   opens it again; a port another program holds gives the in-use message at once.
 *
 * Runs against the wasm demo meter and its CDC twin on Vitest's fake clock.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { MeterSettings, SettingChanges, UiError } from "../backend";
import { CdcPipe, DemoCdcTransport, serialPipeOpener } from "./cdcTransport";
import { DemoTransport } from "./demoTransport";
import { MeterDriver } from "./driver";
import type { PowerzWasm } from "./driver";
import type { DemoCdcLike } from "./fakeSerial.test";
import { FakeSerial, FakeSerialPort, foreignAdcReply, withId } from "./fakeSerial.test";
import { openSerialLink, type SerialLink } from "./serial";
import { loadWasm } from "./testWasm.test";
import { timeoutError, type WebTransport } from "./transport";

type DriverOptions = Parameters<typeof MeterDriver.connect>[0];
type WriteDriver = MeterDriver & {
  writeSettings(
    changes: SettingChanges,
    openPipe: () => Promise<WebTransport>,
  ): Promise<MeterSettings>;
};
type PipeWasm = ConstructorParameters<typeof CdcPipe>[0];
type OpenerWasm = Parameters<typeof serialPipeOpener>[1];

const IN_USE: UiError = {
  kind: "device_busy",
  message: "The meter's serial port is in use by another program.",
  hint: "Close that program, then try again.",
};

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

async function rejection(p: Promise<unknown>, maxMs?: number): Promise<UiError> {
  try {
    await settle(p, maxMs);
  } catch (e) {
    return e as UiError;
  }
  throw new Error("the call resolved");
}

interface Run {
  demo: DemoTransport;
  driver: WriteDriver;
  errors: UiError[];
}

async function start(): Promise<Run> {
  const demo = new DemoTransport(w, "load-5v-1a", () => Date.now());
  const errors: UiError[] = [];
  const options = {
    wasm: w,
    transport: demo,
    rateCode: 2,
    hid: false,
    onBatch: () => {},
    onError: (e: UiError) => errors.push(e),
    random: () => 0x1234_5678_9abc_def0n,
  } as unknown as DriverOptions;
  const driver = (await settle(MeterDriver.connect(options))) as WriteDriver;
  await vi.advanceTimersByTimeAsync(250);
  return { demo, driver, errors };
}

const valueOf = (s: MeterSettings, key: string) => s.fields.find((f) => f.key === key)?.value;

const isSettingsRead = (f: Uint8Array) =>
  f.length === 4 && f[0] === 0x0c && f[2] === 0x10 && f[3] === 0x00;
const isWrite = (f: Uint8Array) => ((f[0] ?? 0) & 0x7f) === 0x0a && f.length > 4;
const kinds = (frames: Uint8Array[]) =>
  frames.map((f) => (isSettingsRead(f) ? "read" : isWrite(f) ? "write" : "other"));

const enc = (s: string) => new TextEncoder().encode(s);

// ---------------------------------------------------------------------------
// The ADCNEW stream on the serial port (docs/protocol.md "CDC and the ADCNEW
// stream"): `02 att obj crc` + obj × 4 body bytes, CRC-8 poly 0x29 init 0 over the
// body, 20-byte records, att 0x10 | rate.
// ---------------------------------------------------------------------------

function crc8(body: Uint8Array): number {
  let c = 0;
  for (const b of body) {
    c ^= b;
    for (let i = 0; i < 8; i++) c = c & 0x80 ? ((c << 1) ^ 0x29) & 0xff : (c << 1) & 0xff;
  }
  return c;
}

/** A record frame of `records` 20-byte records at rate code 2. Every record starts
 * with 0x41 (the PUT_DATA type byte) at a 4-byte aligned offset: the bare CDC frame
 * length cut such a frame into 4-byte "frames" and took one for a data reply. The
 * other record bytes are neither text nor a reply type. */
function streamFrame(records: number, seed: number): Uint8Array {
  const body = new Uint8Array(records * 20);
  for (let r = 0; r < records; r++) {
    const at = r * 20;
    body[at] = 0x41;
    body[at + 1] = (seed + r) & 0xff;
    body[at + 2] = 0x0b;
    body[at + 3] = 0x00;
    for (let i = 4; i < 20; i++) body[at + i] = 0xa0 | (i & 0x0f);
  }
  const frame = new Uint8Array(4 + body.length);
  frame.set([0x02, 0x12, body.length / 4, crc8(body)]);
  frame.set(body, 4);
  return frame;
}

/** The last `n` bytes of a stream frame whose start went to the earlier session. */
function partialTail(n: number): Uint8Array {
  const frame = streamFrame(2, 0x30);
  return frame.slice(frame.length - n);
}

/** `reply` in 64-byte USB packets; with `frame`, that frame sits between the
 * packets at byte 128 (inside the reply). */
function packets(reply: Uint8Array, frame?: Uint8Array): Uint8Array[] {
  const out: Uint8Array[] = [];
  for (let at = 0; at < reply.length; at += 64) {
    if (frame && at === 128) out.push(frame);
    out.push(reply.slice(at, at + 64));
  }
  return out;
}

/** A demo SETTINGS reply with block B spoiled (its block check fails). */
function spoil(reply: Uint8Array): Uint8Array {
  const out = reply.slice();
  out[150] = (out[150] ?? 0) ^ 0x5a;
  return out;
}

// ---------------------------------------------------------------------------
// AC-X3H3JY.1: a meter-like serial port.
// ---------------------------------------------------------------------------

/** The meter's serial port while the browser streams over WebUSB: stream frames
 * every 40 ms and around every reply, a partial frame first after each open (the
 * tail of one the earlier session never read), the 2nd SETTINGS reply of the first
 * session and the read-back of the second split by a stream frame inside it. */
class MeterPort extends FakeSerialPort {
  sessions = 0;
  private twin: DemoCdcLike;
  private timer: ReturnType<typeof setInterval> | undefined;
  private seed = 1;
  private readsThisSession = 0;
  private wroteThisSession = false;
  private splitReadBack = false;

  constructor(private demo: DemoTransport) {
    super();
    this.twin = demo.cdcTwin();
    this.respond = (frame) => this.answer(frame);
  }

  private frame(): Uint8Array {
    this.seed = (this.seed + 3) & 0xff;
    return streamFrame(2, this.seed);
  }

  override async open(options: { baudRate: number }): Promise<void> {
    await super.open(options);
    this.sessions++;
    this.readsThisSession = 0;
    this.wroteThisSession = false;
    this.emit(partialTail(13));
    this.emit(this.frame());
    this.timer = setInterval(() => this.emit(this.frame()), 40);
  }

  override async close(): Promise<void> {
    await super.close();
    clearInterval(this.timer);
  }

  private answer(request: Uint8Array): Uint8Array[] {
    this.demo.syncTo(Date.now());
    const reply = this.twin.transact(request);
    if (isWrite(request)) this.wroteThisSession = true;
    let inside: Uint8Array | undefined;
    if (isSettingsRead(request)) {
      this.readsThisSession++;
      if (this.sessions === 1 && this.readsThisSession === 2) inside = this.frame();
      if (this.sessions === 2 && this.wroteThisSession && !this.splitReadBack) {
        this.splitReadBack = true;
        inside = this.frame();
      }
    }
    if (reply.length === 0) return [this.frame()];
    return [this.frame(), ...packets(reply, inside), this.frame()];
  }
}

describe("two changes back to back on a streaming meter's port (AC-X3H3JY.1)", () => {
  it("both succeed: 50 → 60, then 60 → 50, each confirmed by its read-back", async () => {
    const { demo, driver, errors } = await start();
    const port = new MeterPort(demo);
    const serial = new FakeSerial();
    serial.ports = [port];
    const open = serialPipeOpener(serial, w as unknown as OpenerWasm);

    const first = await settle(driver.writeSettings({ brightness: 60 }, open));
    expect(valueOf(first, "brightness")).toContain("60");
    expect(first.warnings ?? []).toEqual([]);

    await vi.advanceTimersByTimeAsync(3_000);

    const second = await settle(driver.writeSettings({ brightness: 50 }, open));
    expect(valueOf(second, "brightness")).toContain("50");
    expect(second.warnings ?? []).toEqual([]);

    // One write per change, never sent twice; the same granted port both times.
    expect(kinds(port.frames()).filter((k) => k === "write")).toHaveLength(2);
    expect(kinds(port.frames())).not.toContain("other");
    expect(port.sessions).toBe(2);
    expect(serial.requests).toEqual([]);
    expect(port.isOpen).toBe(false);
    expect(valueOf(await settle(driver.readSettings()), "brightness")).toContain("50");
    expect(errors).toEqual([]);
    await settle(driver.disconnect());
  });
});

// ---------------------------------------------------------------------------
// AC-X3H3JY.2: what counts as another program's reply.
// ---------------------------------------------------------------------------

/** A serial link holding `held` chunks; each write gets `respond`'s chunks. */
class ChunkLink implements SerialLink {
  written: Uint8Array[] = [];
  closed = false;
  private chunks: Uint8Array[];
  constructor(
    held: Uint8Array[],
    private respond: (frame: Uint8Array) => Uint8Array[],
  ) {
    this.chunks = [...held];
  }
  async write(bytes: Uint8Array): Promise<void> {
    this.written.push(bytes.slice());
    this.chunks.push(...this.respond(bytes));
  }
  async read(timeoutMs: number): Promise<Uint8Array> {
    const next = this.chunks.shift();
    if (next) return next;
    if (timeoutMs <= 0) throw timeoutError();
    await new Promise((r) => setTimeout(r, timeoutMs));
    throw timeoutError();
  }
  drain(): void {
    this.chunks = [];
  }
  async close(): Promise<void> {
    this.closed = true;
  }
}

/** The demo twin's reply to `frame`, from a fresh twin of `demo`. */
function twinAnswer(demo: DemoTransport): (frame: Uint8Array) => Uint8Array {
  const twin = demo.cdcTwin();
  return (frame) => {
    demo.syncTo(Date.now());
    return twin.transact(frame);
  };
}

const NOISE = new Uint8Array([0x99, 0x93, 0xa7, 0x00, 0xb5, 0xc8]);

describe("stream frames, text and noise on the settings port (AC-X3H3JY.2)", () => {
  const pipeOn = (link: ChunkLink) => new CdcPipe(w as unknown as PipeWasm, link);

  it("held stream frames, a partial frame, text lines and noise are not another program", async () => {
    const demo = new DemoTransport(w, "load-5v-1a", () => Date.now());
    const answer = twinAnswer(demo);
    const link = new ChunkLink(
      [partialTail(13), streamFrame(2, 1), enc("Apple 2.4A\n"), NOISE, streamFrame(3, 9)],
      (f) => [streamFrame(1, 4), ...packets(answer(f)), streamFrame(2, 5)],
    );
    const request = w.settingsRequest(0x21);
    const reply = await settle(pipeOn(link).transact(request, 1_000));
    expect(link.written).toEqual([request]);
    expect(reply).toEqual(withId(answer(w.settingsRequest(0x21)), 0x21));
  });

  it("stream frames, text and noise ahead of the reply after the write are skipped", async () => {
    const demo = new DemoTransport(w, "load-5v-1a", () => Date.now());
    const answer = twinAnswer(demo);
    const link = new ChunkLink([], (f) => [
      streamFrame(2, 0x41),
      enc("OK\n"),
      NOISE,
      streamFrame(1, 0x42),
      enc("Apple 2.4A\n"),
      ...packets(answer(f)),
    ]);
    const reply = await settle(pipeOn(link).transact(w.settingsRequest(0x22), 1_000));
    expect(reply[0]).toBe(0x41);
    expect(reply[1]).toBe(0x22);
    expect(w.decodeSettings(reply).fields.length).toBeGreaterThan(5);
  });

  it("only stream frames and text: a timeout, not the in-use message", async () => {
    const link = new ChunkLink([streamFrame(2, 1), enc("pdm open\n")], () => [
      streamFrame(2, 2),
      enc("Apple 2.4A\n"),
      streamFrame(3, 3),
      NOISE,
    ]);
    const e = await rejection(pipeOn(link).transact(w.settingsRequest(0x23), 500));
    expect(e.kind).toBe("timeout");
  });

  it("a whole data reply for another attribute among held stream frames stops before the write", async () => {
    const link = new ChunkLink(
      [streamFrame(2, 1), foreignAdcReply(w, 0x5a), streamFrame(2, 2), enc("OK\n")],
      () => [],
    );
    const e = await rejection(pipeOn(link).transact(w.settingsRequest(0x24), 1_000));
    expect(e).toEqual(IN_USE);
    expect(link.written).toEqual([]);
  });

  it("a whole data reply for another attribute after the write still gives the in-use message", async () => {
    const link = new ChunkLink([], () => [
      streamFrame(1, 7),
      ...packets(foreignAdcReply(w, 0x5b)),
      streamFrame(1, 8),
    ]);
    const e = await rejection(pipeOn(link).transact(w.settingsRequest(0x25), 1_000));
    expect(e).toEqual(IN_USE);
  });

  it("a change over a port full of stream frames, text and noise reads, writes and reads back", async () => {
    const { demo, driver, errors } = await start();
    const answer = twinAnswer(demo);
    const link = new ChunkLink([partialTail(9), streamFrame(2, 1), enc("Apple 2.4A\n")], (f) => {
      const reply = answer(f);
      return [streamFrame(2, f[1] ?? 0), NOISE, enc("OK\n"), ...packets(reply), streamFrame(1, 3)];
    });
    const after = await settle(driver.writeSettings({ brightness: 70 }, async () => pipeOn(link)));
    expect(valueOf(after, "brightness")).toContain("70");
    expect(kinds(link.written)).toEqual(["read", "read", "write", "read"]);
    expect(link.closed).toBe(true);
    expect(errors).toEqual([]);
    await settle(driver.disconnect());
  });

  it("another program's reply among stream frames still stops the change before the write", async () => {
    const { demo, driver } = await start();
    const answer = twinAnswer(demo);
    const link = new ChunkLink([], (f) => [
      streamFrame(2, 1),
      foreignAdcReply(w, 0x5a),
      streamFrame(2, 2),
      ...packets(answer(f)),
    ]);
    const e = await rejection(driver.writeSettings({ brightness: 80 }, async () => pipeOn(link)));
    expect(e).toEqual(IN_USE);
    expect(kinds(link.written)).not.toContain("write");
    expect(valueOf(await settle(driver.readSettings()), "brightness")).not.toContain("80");
    await settle(driver.disconnect());
  });
});

// ---------------------------------------------------------------------------
// AC-X3H3JY.3: a SETTINGS reply that fails its block check is read again.
// ---------------------------------------------------------------------------

/** A settings pipe over the demo twin whose SETTINGS replies to the requests listed
 * in `spoiled` (1-based, counting every SETTINGS request) fail their block check;
 * with `ignoreWrite` the meter neither takes nor answers the write. */
class SpoilingPipe implements WebTransport {
  sent: Uint8Array[] = [];
  closed = false;
  ignoreWrite = false;
  private reads = 0;
  private inner: DemoCdcTransport;
  constructor(
    demo: DemoTransport,
    private spoiled: number[],
  ) {
    this.inner = new DemoCdcTransport(demo, () => Date.now());
  }
  async transact(frame: Uint8Array, timeoutMs: number): Promise<Uint8Array> {
    this.sent.push(frame.slice());
    if (isWrite(frame) && this.ignoreWrite) {
      await new Promise((r) => setTimeout(r, timeoutMs));
      throw timeoutError();
    }
    const reply = await this.inner.transact(frame, timeoutMs);
    if (!isSettingsRead(frame)) return reply;
    this.reads++;
    return this.spoiled.includes(this.reads) ? spoil(reply) : reply;
  }
  async read(timeoutMs: number): Promise<Uint8Array> {
    await new Promise((r) => setTimeout(r, timeoutMs));
    throw timeoutError();
  }
  async close(): Promise<void> {
    this.closed = true;
    await this.inner.close();
  }
}

const readIds = (frames: Uint8Array[]) => frames.filter(isSettingsRead).map((f) => f[1]);

describe("a SETTINGS reply that fails its block check (AC-X3H3JY.3)", () => {
  it("the spoiled reply's own check really fails", () => {
    const demo = new DemoTransport(w, "load-5v-1a", () => Date.now());
    const reply = twinAnswer(demo)(w.settingsRequest(1));
    expect(() => w.decodeSettings(spoil(reply))).toThrow();
  });

  it("a spoiled first read is read again with a new request, and the change takes", async () => {
    const { demo, driver, errors } = await start();
    const pipe = new SpoilingPipe(demo, [1]);
    const after = await settle(driver.writeSettings({ brightness: 60 }, async () => pipe));
    expect(valueOf(after, "brightness")).toContain("60");
    expect(kinds(pipe.sent)).toEqual(["read", "read", "read", "write", "read"]);
    const ids = readIds(pipe.sent);
    expect(new Set(ids).size).toBe(ids.length);
    expect(pipe.closed).toBe(true);
    expect(errors).toEqual([]);
    await settle(driver.disconnect());
  });

  it("a second read spoiled twice takes its third read (3 reads in all for that step)", async () => {
    const { demo, driver } = await start();
    const pipe = new SpoilingPipe(demo, [2, 3]);
    const after = await settle(driver.writeSettings({ brightness: 60 }, async () => pipe));
    expect(valueOf(after, "brightness")).toContain("60");
    expect(kinds(pipe.sent)).toEqual(["read", "read", "read", "read", "write", "read"]);
    await settle(driver.disconnect());
  });

  it("three spoiled reads for one step fail the change and nothing is written", async () => {
    const { demo, driver } = await start();
    const pipe = new SpoilingPipe(demo, [1, 2, 3, 4, 5]);
    const e = await rejection(driver.writeSettings({ brightness: 60 }, async () => pipe));
    expect(e.message).toMatch(/CRC mismatch/);
    expect(kinds(pipe.sent)).toEqual(["read", "read", "read"]);
    expect(pipe.closed).toBe(true);
    expect(valueOf(await settle(driver.readSettings()), "brightness")).not.toContain("60");
    await settle(driver.disconnect());
  });

  it("a spoiled read-back is read again; one that shows the change reports success", async () => {
    const { demo, driver, errors } = await start();
    const pipe = new SpoilingPipe(demo, [3]);
    const after = await settle(driver.writeSettings({ brightness: 60 }, async () => pipe));
    expect(valueOf(after, "brightness")).toContain("60");
    expect(after.warnings ?? []).toEqual([]);
    expect(kinds(pipe.sent)).toEqual(["read", "read", "write", "read", "read"]);
    expect(errors).toEqual([]);
    await settle(driver.disconnect());
  });

  it("a read-back spoiled three times fails, and the write was sent once", async () => {
    const { demo, driver } = await start();
    const pipe = new SpoilingPipe(demo, [3, 4, 5]);
    const e = await rejection(driver.writeSettings({ brightness: 60 }, async () => pipe));
    expect(e.message).toMatch(/CRC mismatch/);
    expect(kinds(pipe.sent)).toEqual(["read", "read", "write", "read", "read", "read"]);
    await settle(driver.disconnect());
  });

  it("a read-back read again that does not show the change still fails naming the field", async () => {
    const { demo, driver } = await start();
    const pipe = new SpoilingPipe(demo, [3]);
    pipe.ignoreWrite = true;
    const e = await rejection(driver.writeSettings({ brightness: 60 }, async () => pipe));
    expect(e.kind).toBe("protocol");
    expect(e.message).toMatch(
      /^The meter's settings do not match after the change: brightness reads .+, expected 60\.$/,
    );
    expect(e.hint).toBe("Read the settings again; nothing was retried.");
    expect(kinds(pipe.sent)).toEqual(["read", "read", "write", "read", "read"]);
    await settle(driver.disconnect());
  });
});

// ---------------------------------------------------------------------------
// AC-X3H3JY.4: a port this page still holds open.
// ---------------------------------------------------------------------------

/** A port whose first `failCloses` closes do not finish: the port stays open with
 * its (released) streams. */
class StickyPort extends FakeSerialPort {
  failCloses = 0;
  override async close(): Promise<void> {
    if (this.failCloses > 0 && this.isOpen) {
      this.failCloses--;
      throw new DOMException("The port could not be closed.", "NetworkError");
    }
    await super.close();
  }
}

describe("opening a port this page still holds open (AC-X3H3JY.4)", () => {
  it("closes it and opens it again, and the link works", async () => {
    const port = new FakeSerialPort((f) => [f.slice()]);
    await port.open({ baudRate: 115_200 });
    expect(port.readable).not.toBeNull();
    const link = await settle(openSerialLink(port));
    expect(port.opens).toHaveLength(2);
    expect(port.closes).toBe(1);
    expect(port.isOpen).toBe(true);
    await settle(link.write(new Uint8Array([1, 2, 3, 4])));
    expect([...(await settle(link.read(500)))]).toEqual([1, 2, 3, 4]);
    await settle(link.close());
    expect(port.isOpen).toBe(false);
  });

  it("a port another program holds gives the in-use message at once", async () => {
    const port = new FakeSerialPort();
    port.openError = new DOMException("Failed to open serial port.", "InvalidStateError");
    let error: unknown = null;
    await openSerialLink(port).catch((e: unknown) => {
      error = e;
    });
    expect(error).toEqual(IN_USE);
    expect(port.opens).toHaveLength(1);
    expect(port.closes).toBe(0);
  });

  it("a change right after one whose close did not finish takes on the same port", async () => {
    const { demo, driver, errors } = await start();
    const answer = twinAnswer(demo);
    const port = new StickyPort((f) => {
      const reply = answer(f);
      return reply.length > 0 ? [reply] : [];
    });
    port.failCloses = 1;
    const serial = new FakeSerial();
    serial.ports = [port];
    const open = serialPipeOpener(serial, w as unknown as OpenerWasm);

    const first = await settle(driver.writeSettings({ brightness: 60 }, open));
    expect(valueOf(first, "brightness")).toContain("60");
    // The close did not finish: this page still holds the port.
    expect(port.isOpen).toBe(true);

    const second = await settle(driver.writeSettings({ brightness: 50 }, open));
    expect(valueOf(second, "brightness")).toContain("50");
    expect(kinds(port.frames()).filter((k) => k === "write")).toHaveLength(2);
    expect(serial.requests).toEqual([]);
    expect(port.isOpen).toBe(false);
    expect(errors).toEqual([]);
    await settle(driver.disconnect());
  });
});
