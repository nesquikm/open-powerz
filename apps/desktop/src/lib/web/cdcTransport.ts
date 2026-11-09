/**
 * FR W8SHX9: the pipe a settings write runs over, on the meter's serial port.
 *
 * `CdcPipe` is a `WebTransport` over a Web Serial link: it sends only GET_DATA SETTINGS
 * reads and the type-0x0A settings write (never the ADCNEW control frames 0x01..0x03,
 * AC-W8SHX9.6) and frames the replies with the wasm core's CDC framing.
 * `DemoCdcTransport` is the same allowlist over the demo meter's CDC twin.
 */

import type { DemoTransport } from "./demoTransport";
import type { PowerzWasm } from "./driver";
import {
  noWebSerialError,
  openSerialLink,
  pickMeterPort,
  portHeldError,
  type SerialLike,
  type SerialLink,
} from "./serial";
import { disconnectedError, notSafeError, timeoutError, type WebTransport } from "./transport";

/** The settings write's frame type. */
const SETTINGS_WRITE_TYPE = 0x0a;

/** GET_DATA SETTINGS (`0c id 10 00`) or a settings write: the only frames the
 * settings pipe sends. */
export function isSettingsFrame(frame: Uint8Array): boolean {
  const ty = (frame[0] ?? 0) & 0x7f;
  if (ty === SETTINGS_WRITE_TYPE) return frame.length > 4;
  return frame.length === 4 && frame[0] === 0x0c && frame[2] === 0x10 && frame[3] === 0x00;
}

/** PUT_DATA, the type of every data reply. */
const PUT_DATA_TYPE = 0x41;
/** The SETTINGS attribute (`ATT_SETTINGS`), the only data the settings pipe asks for. */
const SETTINGS_ATTRIBUTE = 0x008;

/** FR CJ3QC9: a data reply for another attribute than SETTINGS, which can only answer
 * another program's request on the same port. */
export function isForeignReply(frame: Uint8Array): boolean {
  if (((frame[0] ?? 0) & 0x7f) !== PUT_DATA_TYPE || frame.length < 6) return false;
  const attribute = ((frame[4] ?? 0) | ((frame[5] ?? 0) << 8)) & 0x7fff;
  return attribute !== SETTINGS_ATTRIBUTE;
}

/** `parts` joined into one array. */
function concat(parts: Uint8Array[]): Uint8Array<ArrayBuffer> {
  const out = new Uint8Array(parts.reduce((n, p) => n + p.length, 0));
  let at = 0;
  for (const p of parts) {
    out.set(p, at);
    at += p.length;
  }
  return out;
}

/** The wasm export that frames CDC replies. */
export interface CdcFraming {
  cdcFrameLength(buf: Uint8Array): number | undefined;
}

/** The settings pipe over a Web Serial link. */
export class CdcPipe implements WebTransport {
  private buffer = new Uint8Array(0);

  constructor(
    private readonly wasm: CdcFraming,
    private readonly link: SerialLink,
  ) {}

  async transact(frame: Uint8Array, timeoutMs: number): Promise<Uint8Array> {
    if (!isSettingsFrame(frame)) throw notSafeError();
    // Bytes still held answer an earlier request that timed out (native CDC
    // `drain_demux` before a request): drop them.
    // FR CJ3QC9: another program's replies among them mean it shares the port.
    const held = this.link.drain ? await this.takeHeld() : new Uint8Array(0);
    this.link.drain?.();
    this.buffer = new Uint8Array(0);
    if (this.holdsForeignReply(held)) throw portHeldError();
    await this.link.write(frame);
    return this.read(timeoutMs);
  }

  /** The next complete frame; rejects `timeout` when none completes in `timeoutMs`,
   * and with the in-use error on a reply the settings pipe never asks for (FR CJ3QC9). */
  async read(timeoutMs: number): Promise<Uint8Array> {
    const deadline = Date.now() + timeoutMs;
    for (;;) {
      const n = this.buffer.length > 0 ? this.wasm.cdcFrameLength(this.buffer) : undefined;
      if (n !== undefined && n > 0) {
        const out = this.buffer.slice(0, n);
        this.buffer = this.buffer.slice(n);
        if (isForeignReply(out)) throw portHeldError();
        return out;
      }
      const left = deadline - Date.now();
      if (left <= 0) throw timeoutError();
      this.buffer = concat([this.buffer, await this.link.read(left)]);
    }
  }

  /** The buffer plus every byte the link already holds, without waiting. */
  private async takeHeld(): Promise<Uint8Array> {
    const parts: Uint8Array[] = [this.buffer];
    for (;;) {
      try {
        parts.push(await this.link.read(0));
      } catch {
        break;
      }
    }
    return concat(parts);
  }

  /** Whether the complete frames in `bytes` include another program's reply. */
  private holdsForeignReply(bytes: Uint8Array): boolean {
    let rest = bytes;
    for (;;) {
      const n = rest.length > 0 ? this.wasm.cdcFrameLength(rest) : undefined;
      if (n === undefined || n <= 0) return false;
      if (isForeignReply(rest.slice(0, n))) return true;
      rest = rest.slice(n);
    }
  }

  async close(): Promise<void> {
    await this.link.close();
  }
}

/** The settings pipe opener of a real meter: the meter's serial port through Web
 * Serial (`serial`, `null` without it), picked on first use and reused once granted. */
export function serialPipeOpener(
  serial: SerialLike | null,
  wasm: Pick<PowerzWasm, "cdcFrameLength">,
): () => Promise<WebTransport> {
  return async () => {
    if (!serial) throw noWebSerialError();
    const port = await pickMeterPort(serial);
    return new CdcPipe(wasm, await openSerialLink(port));
  };
}

/** The demo meter's CDC twin (wasm `DemoCdc`). */
export interface DemoCdcLike {
  transact(request: Uint8Array): Uint8Array;
  advance(ms: number): void;
  free(): void;
}

/** The settings pipe of the demo meter: its CDC twin, sharing its stored settings. */
export class DemoCdcTransport implements WebTransport {
  private twin: DemoCdcLike | null;

  /** `now` must be on the same time base as `demo`'s clock. */
  constructor(
    private readonly demo: DemoTransport,
    private readonly now: () => number = () => performance.now(),
  ) {
    this.twin = demo.cdcTwin();
  }

  async transact(frame: Uint8Array, timeoutMs: number): Promise<Uint8Array> {
    const twin = this.twin;
    if (!twin) throw disconnectedError();
    if (!isSettingsFrame(frame)) throw notSafeError();
    // The twin shares the demo meter's clock: move it once, through the demo meter.
    this.demo.syncTo(this.now());
    const reply = twin.transact(frame);
    if (reply.length > 0) return reply;
    return this.timeOut(timeoutMs);
  }

  async read(timeoutMs: number): Promise<Uint8Array> {
    if (!this.twin) throw disconnectedError();
    return this.timeOut(timeoutMs);
  }

  private async timeOut(timeoutMs: number): Promise<never> {
    await new Promise((r) => setTimeout(r, timeoutMs));
    throw this.twin ? timeoutError() : disconnectedError();
  }

  async close(): Promise<void> {
    this.twin?.free();
    this.twin = null;
  }
}
