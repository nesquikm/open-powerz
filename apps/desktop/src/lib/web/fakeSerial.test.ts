/**
 * FR W8SHX9: hand-written fake Web Serial objects (no mocking library).
 *
 * - `FakeSerialPort`: a `SerialPort` with `open({ baudRate })`, `close()`, in-memory
 *   `readable` / `writable` streams and `getInfo()`. Every chunk written is kept with the
 *   fake clock's time; the responder's chunks for it come back on `readable`. Like a real
 *   port, `close()` refuses while a stream is locked, and `open()` refuses an open port.
 * - `FakeSerial`: a `navigator.serial` with `getPorts()` (granted ports) and
 *   `requestPort({ filters })` (the picker; `pick === null` cancels it).
 * - `demoCdcResponder`: answers like the wasm demo meter's CDC twin, its clock driven by
 *   `Date.now()`.
 *
 * Exported for the serial, CDC pipe, driver and backend tests; the tests below check
 * the fakes themselves.
 */

import { describe, expect, it } from "vitest";
import type { CdcReplyReaderLike } from "./cdcTransport";
import type { PowerzWasm } from "./driver";
import { loadWasm } from "./testWasm.test";

export type SerialResponder = (frame: Uint8Array) => Uint8Array[];

/** The POWER-Z vendor id, as Web Serial's `usbVendorId`. */
export const POWERZ_USB_VENDOR = 0x5fc9;

export class FakeSerialPort {
  info: { usbVendorId?: number; usbProductId?: number } = {
    usbVendorId: POWERZ_USB_VENDOR,
    usbProductId: 0x0063,
  };
  /** Every `open` call's options, in order. */
  opens: { baudRate: number }[] = [];
  /** What the next `open` throws (e.g. a busy port's `InvalidStateError`). */
  openError: unknown = null;
  isOpen = false;
  closes = 0;
  /** Every chunk written, with `Date.now()` at the time. */
  written: { at: number; data: Uint8Array }[] = [];
  readable: ReadableStream<Uint8Array> | null = null;
  writable: WritableStream<Uint8Array> | null = null;
  private controller: ReadableStreamDefaultController<Uint8Array> | null = null;

  constructor(public respond: SerialResponder = () => []) {}

  getInfo(): { usbVendorId?: number; usbProductId?: number } {
    return { ...this.info };
  }

  async open(options: { baudRate: number }): Promise<void> {
    this.opens.push({ ...options });
    if (this.openError) throw this.openError;
    if (this.isOpen) throw new DOMException("The port is already open.", "InvalidStateError");
    this.isOpen = true;
    this.readable = new ReadableStream<Uint8Array>({
      start: (c) => {
        this.controller = c;
      },
      cancel: () => {
        this.controller = null;
      },
    });
    this.writable = new WritableStream<Uint8Array>({
      write: (chunk) => {
        const data = new Uint8Array(chunk);
        this.written.push({ at: Date.now(), data });
        const replies = this.respond(data);
        // The meter answers after the write completes, not inside it.
        void Promise.resolve().then(() => {
          for (const r of replies) this.emit(r);
        });
      },
    });
  }

  /** Bytes arriving from the meter. */
  emit(bytes: Uint8Array): void {
    try {
      this.controller?.enqueue(bytes.slice());
    } catch {
      // The reader was cancelled meanwhile.
    }
  }

  async close(): Promise<void> {
    if (!this.isOpen) throw new DOMException("The port is not open.", "InvalidStateError");
    if (this.readable?.locked || this.writable?.locked) {
      throw new TypeError("Cannot close a port while its streams are locked.");
    }
    this.isOpen = false;
    this.closes++;
    this.readable = null;
    this.writable = null;
    this.controller = null;
  }

  /** The frames written so far (each write is one frame). */
  frames(): Uint8Array[] {
    return this.written.map((w) => w.data);
  }
}

export class FakeSerial {
  /** Ports the page already has permission for (what `getPorts()` answers). */
  ports: FakeSerialPort[] = [];
  /** Every `requestPort` call's options, in order. */
  requests: unknown[] = [];
  /** What the picker returns next; `null`: the user cancels it. */
  pick: FakeSerialPort | null = null;

  async getPorts(): Promise<FakeSerialPort[]> {
    return [...this.ports];
  }

  requestPort(options: unknown): Promise<FakeSerialPort> {
    this.requests.push(options);
    const picked = this.pick;
    if (!picked) {
      return Promise.reject(new DOMException("No port selected by the user.", "NotFoundError"));
    }
    if (!this.ports.includes(picked)) this.ports.push(picked);
    return Promise.resolve(picked);
  }
}

/** The wasm demo CDC twin (FR W8SHX9). */
export interface DemoCdcLike {
  transact(request: Uint8Array): Uint8Array;
  advance(ms: number): void;
  free(): void;
}

/** A demo meter with its CDC twin. */
export interface DemoMeterWithTwin {
  transact(request: Uint8Array): Uint8Array;
  advance(ms: number): void;
  cdcTwin(): DemoCdcLike;
  free(): void;
}

/** A responder answering like `twin`, its clock following `Date.now()`. */
export function demoCdcResponder(twin: DemoCdcLike): SerialResponder {
  let last = Date.now();
  return (frame) => {
    const now = Date.now();
    if (now > last) {
      twin.advance(now - last);
      last = now;
    }
    const reply = twin.transact(frame);
    return reply.length > 0 ? [reply] : [];
  };
}

/** A fresh demo meter (`load-5v-1a`) with the FR W8SHX9 twin. */
export function demoMeterWithTwin(w: PowerzWasm, scenario = "load-5v-1a"): DemoMeterWithTwin {
  return new w.DemoMeter(scenario) as unknown as DemoMeterWithTwin;
}

/** The wasm `SettingsWrite` (FR W8SHX9): one settings write between two reads. */
export interface SettingsWriteLike {
  readRequest(id: number): Uint8Array;
  takeRead(reply: Uint8Array): void;
  writeRequest(id: number): Uint8Array;
  takeWriteReply(reply: Uint8Array | undefined): void;
  result(): { fields: { key: string; value: string }[]; warnings: string[] };
  free(): void;
}

/** The FR W8SHX9 wasm exports. */
export interface WriteWasm {
  SettingsWrite: new (changes: Record<string, number>) => SettingsWriteLike;
  cdcFrameLength(buf: Uint8Array): number | undefined;
  CdcReplyReader: new () => CdcReplyReaderLike;
}

export const writeWasm = (w: PowerzWasm): WriteWasm => w as unknown as WriteWasm;

/** Write `changes` through `twin` with the wasm `SettingsWrite` (ids 1..4). */
function writeThrough(w: PowerzWasm, twin: DemoCdcLike, changes: Record<string, number>): void {
  const sw = new (writeWasm(w).SettingsWrite)(changes);
  try {
    sw.takeRead(twin.transact(sw.readRequest(1)));
    sw.takeRead(twin.transact(sw.readRequest(2)));
    const reply = twin.transact(sw.writeRequest(3));
    sw.takeWriteReply(reply.length > 0 ? reply : undefined);
    sw.takeRead(twin.transact(sw.readRequest(4)));
    sw.result();
  } finally {
    sw.free();
  }
}

/** A SETTINGS reply of a fresh demo meter's twin, after `changes` were written there
 * (none: as the meter opens). */
export function demoSettingsReply(w: PowerzWasm, changes?: Record<string, number>): Uint8Array {
  const meter = demoMeterWithTwin(w);
  const twin = meter.cdcTwin();
  try {
    if (changes) writeThrough(w, twin, changes);
    return twin.transact(w.settingsRequest(9));
  } finally {
    twin.free();
    meter.free();
  }
}

/** The settings write frame (id `id`) of `changes` over a fresh demo meter's reads. */
export function demoWriteFrame(
  w: PowerzWasm,
  changes: Record<string, number>,
  id: number,
): Uint8Array {
  const reply = demoSettingsReply(w);
  const sw = new (writeWasm(w).SettingsWrite)(changes);
  try {
    sw.takeRead(reply);
    sw.takeRead(reply);
    return sw.writeRequest(id);
  } finally {
    sw.free();
  }
}

/** FR CJ3QC9: another program's traffic on the meter's serial port: the PUT_DATA ADC
 * reply (attribute 0x0001) a fresh demo meter gives to a GET_DATA ADC with id `id`,
 * a reply the settings change never asks for. */
export function foreignAdcReply(w: PowerzWasm, id: number): Uint8Array {
  const meter = demoMeterWithTwin(w);
  try {
    const reply = meter.transact(w.getDataRequest(id, 0x001));
    const attribute = ((reply[4] ?? 0) | ((reply[5] ?? 0) << 8)) & 0x7fff;
    if (((reply[0] ?? 0) & 0x7f) !== 0x41 || attribute !== 0x001) {
      throw new Error("the demo meter did not answer GET_DATA ADC with a PUT_DATA ADC reply");
    }
    return withId(reply, id);
  } finally {
    meter.free();
  }
}

/** `reply` answering request id `id` (byte 1 of the main header). */
export function withId(reply: Uint8Array, id: number): Uint8Array {
  const out = reply.slice();
  out[1] = id;
  return out;
}

describe("fake Web Serial", () => {
  it("the picker records its options and adds the picked port; a cancel is NotFoundError", async () => {
    const serial = new FakeSerial();
    const port = new FakeSerialPort();
    serial.pick = port;
    const p = serial.requestPort({ filters: [{ usbVendorId: POWERZ_USB_VENDOR }] });
    expect(serial.requests).toEqual([{ filters: [{ usbVendorId: POWERZ_USB_VENDOR }] }]);
    expect(await p).toBe(port);
    expect(await serial.getPorts()).toEqual([port]);
    serial.pick = null;
    await expect(serial.requestPort({})).rejects.toMatchObject({ name: "NotFoundError" });
  });

  it("a port echoes its responder's chunks and refuses to close while locked", async () => {
    const port = new FakeSerialPort((f) => [f.slice(0, 2), f.slice(2)]);
    await port.open({ baudRate: 115200 });
    const reader = (port.readable as ReadableStream<Uint8Array>).getReader();
    const writer = (port.writable as WritableStream<Uint8Array>).getWriter();
    await writer.write(new Uint8Array([1, 2, 3, 4]));
    const a = await reader.read();
    const b = await reader.read();
    expect([...(a.value ?? []), ...(b.value ?? [])]).toEqual([1, 2, 3, 4]);
    await expect(port.close()).rejects.toBeInstanceOf(TypeError);
    writer.releaseLock();
    await reader.cancel();
    reader.releaseLock();
    await port.close();
    expect(port.closes).toBe(1);
    expect(port.frames()).toEqual([new Uint8Array([1, 2, 3, 4])]);
  });
});

describe("demo CDC twin helper", () => {
  it("the wasm demo meter has a CDC twin that answers a SETTINGS read", async () => {
    const w = await loadWasm();
    const meter = demoMeterWithTwin(w);
    expect(typeof meter.cdcTwin).toBe("function");
    const twin = meter.cdcTwin();
    const reply = demoCdcResponder(twin)(w.settingsRequest(1));
    expect(reply).toHaveLength(1);
    expect(w.decodeSettings(reply[0] as Uint8Array).fields.length).toBeGreaterThan(5);
    twin.free();
    meter.free();
  });
});
