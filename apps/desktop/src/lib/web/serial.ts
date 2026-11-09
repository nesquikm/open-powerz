/**
 * FR W8SHX9: the Web Serial link to the meter's virtual serial port, where the meter
 * takes a settings change. `pickMeterPort` reuses the one granted POWER-Z port or asks
 * the browser for it; `openSerialLink` opens it and moves raw bytes. Every failure
 * rejects with a `UiError`-shaped value.
 */

import type { UiError } from "../backend";
import { disconnectedError, timeoutError, transportError } from "./transport";
import { POWERZ_VENDOR_ID } from "./webusb";

/** The parts of a Web Serial `SerialPort` the link uses. */
export interface SerialPortLike {
  open(options: { baudRate: number }): Promise<void>;
  close(): Promise<void>;
  readonly readable: ReadableStream<Uint8Array> | null;
  readonly writable: WritableStream<Uint8Array> | null;
  getInfo(): { usbVendorId?: number; usbProductId?: number };
}

/** The parts of the browser's Web Serial API (on `navigator`) the picker uses. */
export interface SerialLike {
  getPorts(): Promise<SerialPortLike[]>;
  requestPort(options: { filters: { usbVendorId: number }[] }): Promise<SerialPortLike>;
}

/** The picker filter: POWER-Z ports only. */
export const POWERZ_SERIAL_FILTER = { usbVendorId: POWERZ_VENDOR_ID } as const;

/** The baud rate the port opens at (the meter takes any, `[static]`). */
const BAUD_RATE = 115_200;
/** Wait after opening before the first frame (native `FIRST_FRAME_DELAY`). */
const FIRST_FRAME_DELAY_MS = 200;
/** Longest a write may take before the link counts as gone (`[inferred]`: a 1 kB frame
 * at USB full speed drains in well under that). */
const WRITE_TIMEOUT_MS = 2_000;
/** How long closing waits for the writer to flush before aborting it. */
const CLOSE_FLUSH_MS = 200;
/** Most bytes held between reads: far more than any settings reply. */
export const MAX_BUFFERED_BYTES = 64 * 1024;

const usage = (message: string): UiError => transportError("usage", message, null);

/** No Web Serial in this browser. */
export const noWebSerialError = (): UiError =>
  usage(
    "This browser has no Web Serial: changing settings needs Chrome or Edge on a desktop computer.",
  );

/** Another program holds the port (the desktop app's `port_held`). */
export const portHeldError = (): UiError =>
  transportError(
    "device_busy",
    "The meter's serial port is in use by another program.",
    "Close that program, then try again.",
  );

const errorName = (e: unknown): string | null =>
  typeof e === "object" && e !== null && typeof (e as { name?: unknown }).name === "string"
    ? (e as { name: string }).name
    : null;

/** The meter's serial port: the one granted POWER-Z port, else the browser's picker
 * filtered to POWER-Z ports. `getPorts` makes no picker, so the click's user
 * activation still holds for `requestPort`. A cancelled picker sends nothing. */
export async function pickMeterPort(serial: SerialLike): Promise<SerialPortLike> {
  const granted = (await serial.getPorts()).filter(
    (p) => p.getInfo().usbVendorId === POWERZ_VENDOR_ID,
  );
  if (granted.length === 1 && granted[0]) return granted[0];
  try {
    return await serial.requestPort({ filters: [POWERZ_SERIAL_FILTER] });
  } catch (e) {
    // The browser refuses a picker the user's click did not open.
    if (errorName(e) === "SecurityError") {
      throw usage(
        "The browser did not open the serial port picker, so nothing was sent. Try again.",
      );
    }
    throw usage("No serial port was chosen, so nothing was sent.");
  }
}

/** An open serial link: raw bytes out, raw bytes in. */
export interface SerialLink {
  write(bytes: Uint8Array): Promise<void>;
  /** The bytes that arrived since the last read; rejects `timeout` after `timeoutMs`. */
  read(timeoutMs: number): Promise<Uint8Array>;
  /** Release both streams and close the port. */
  close(): Promise<void>;
  /** Drop the bytes received and not read yet. */
  drain?(): void;
}

/** Open `port` at 115200 baud and wait out the first-frame delay. */
export async function openSerialLink(port: SerialPortLike): Promise<SerialLink> {
  try {
    await port.open({ baudRate: BAUD_RATE });
  } catch (e) {
    const name = errorName(e);
    if (name === "InvalidStateError" || name === "NetworkError") throw portHeldError();
    throw disconnectedError();
  }
  const { readable, writable } = port;
  if (!readable || !writable) {
    await port.close().catch(() => {});
    throw disconnectedError();
  }
  let reader: ReadableStreamDefaultReader<Uint8Array> | null = null;
  try {
    reader = readable.getReader();
    const writer = writable.getWriter();
    const link = new Link(port, reader, writer);
    await new Promise((r) => setTimeout(r, FIRST_FRAME_DELAY_MS));
    return link;
  } catch {
    // A stream still locked from an earlier link: free what was taken, close the port.
    reader?.releaseLock();
    await port.close().catch(() => {});
    throw disconnectedError();
  }
}

/** More bytes than any reply arrived with no frame read from them. */
const overflowError = (): UiError =>
  transportError("protocol", "The meter's serial port sent more data than expected.", null);

class Link implements SerialLink {
  private chunks: Uint8Array[] = [];
  private waiter: (() => void) | null = null;
  private done = false;
  private held = 0;
  private overflowed = false;
  private readonly pump: Promise<void>;

  constructor(
    private readonly port: SerialPortLike,
    private readonly reader: ReadableStreamDefaultReader<Uint8Array>,
    private readonly writer: WritableStreamDefaultWriter<Uint8Array>,
  ) {
    this.pump = this.run();
  }

  private async run(): Promise<void> {
    try {
      for (;;) {
        const { value, done } = await this.reader.read();
        if (done) break;
        if (value && value.length > 0) {
          this.held += value.length;
          if (this.held > MAX_BUFFERED_BYTES) {
            // Nothing is reading this much: stop rather than grow without bound.
            this.overflowed = true;
            break;
          }
          this.chunks.push(value);
        }
        this.wake();
      }
    } catch {
      // The port went away or the reader was cancelled.
    }
    this.done = true;
    this.wake();
  }

  private wake(): void {
    const w = this.waiter;
    this.waiter = null;
    w?.();
  }

  async write(bytes: Uint8Array): Promise<void> {
    if (this.done) throw disconnectedError();
    let timer: ReturnType<typeof setTimeout> | undefined;
    const late = new Promise<never>((_, reject) => {
      timer = setTimeout(() => reject(disconnectedError()), WRITE_TIMEOUT_MS);
    });
    try {
      await Promise.race([this.writer.write(bytes), late]);
    } catch {
      throw disconnectedError();
    } finally {
      clearTimeout(timer);
    }
  }

  drain(): void {
    this.chunks = [];
    this.held = 0;
  }

  async read(timeoutMs: number): Promise<Uint8Array> {
    const deadline = Date.now() + timeoutMs;
    for (;;) {
      const next = this.chunks.shift();
      if (next) {
        this.held -= next.length;
        return next;
      }
      if (this.overflowed) throw overflowError();
      if (this.done) throw disconnectedError();
      const left = deadline - Date.now();
      if (left <= 0) throw timeoutError();
      await new Promise<void>((resolve) => {
        const timer = setTimeout(() => {
          this.waiter = null;
          resolve();
        }, left);
        this.waiter = () => {
          clearTimeout(timer);
          resolve();
        };
      });
    }
  }

  async close(): Promise<void> {
    await this.reader.cancel().catch(() => {});
    await this.pump;
    this.reader.releaseLock();
    // Nothing is left to flush: give the writer a moment, then abort it.
    let timer: ReturnType<typeof setTimeout> | undefined;
    const flushed = await Promise.race([
      this.writer.close().then(
        () => true,
        () => true,
      ),
      new Promise<false>((r) => (timer = setTimeout(() => r(false), CLOSE_FLUSH_MS))),
    ]);
    clearTimeout(timer);
    if (!flushed) await this.writer.abort().catch(() => {});
    this.writer.releaseLock();
    await this.port.close().catch(() => {});
  }
}
