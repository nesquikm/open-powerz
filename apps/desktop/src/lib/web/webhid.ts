/**
 * FR QFTCWZ: the WebHID transport. A request goes out as one 64-byte output report
 * (report ID 0, the frame zero-padded); the reply comes back as 64-byte input reports,
 * assembled up to the length its headers declare.
 */

import type { UiError } from "../backend";
import { disconnectedError, timeoutError, transportError, type WebTransport } from "./transport";

const REPORT_LEN = 64;
const HEADER_LEN = 4;
const PUT_DATA = 0x41;
const ATT_ADC_QUEUE = 0x002;
const ATT_PD_STATUS = 0x020;

/** The parts of the WebHID `HIDDevice` the transport uses. */
export interface HidDeviceLike {
  opened: boolean;
  open(): Promise<void>;
  close(): Promise<void>;
  sendReport(reportId: number, data: BufferSource): Promise<void>;
  addEventListener(type: "inputreport", fn: (event: { data: DataView }) => void): void;
  removeEventListener(type: "inputreport", fn: (event: { data: DataView }) => void): void;
}

function word(buf: Uint8Array, at: number): number | null {
  if (at + 4 > buf.length) return null;
  return (buf[at]! | (buf[at + 1]! << 8) | (buf[at + 2]! << 16) | (buf[at + 3]! << 24)) >>> 0;
}

/** Total bytes a reply's headers declare (main header plus every section). Non-PUT_DATA
 * replies declare only what arrived. */
export function declaredLength(reply: Uint8Array): number {
  if (((reply[0] ?? 0) & 0x7f) !== PUT_DATA) return reply.length;
  let offset = HEADER_LEN;
  for (;;) {
    const w = word(reply, offset);
    if (w === null) return offset + HEADER_LEN;
    const att = w & 0x7fff;
    const next = ((w >>> 15) & 1) === 1;
    const chunk = (w >>> 16) & 0x3f;
    const size = (w >>> 22) & 0x3ff;
    const end = offset + HEADER_LEN + (att === ATT_ADC_QUEUE ? chunk * size : size);
    if (att === ATT_PD_STATUS || !next) return end;
    offset = end;
  }
}

/** Open `device` as a meter over HID. */
export async function openWebHid(device: HidDeviceLike): Promise<WebTransport> {
  if (!device.opened) {
    try {
      await device.open();
    } catch {
      throw transportError(
        "device_busy",
        "The meter could not be opened.",
        "Close other apps that use the meter and connect again.",
      );
    }
  }
  return new WebHidTransport(device);
}

class WebHidTransport implements WebTransport {
  private gone: UiError | null = null;
  private queue: Uint8Array[] = [];
  private wake: (() => void) | null = null;
  private readonly onReport = (event: { data: DataView }): void => {
    const d = event.data;
    this.queue.push(new Uint8Array(d.buffer.slice(d.byteOffset, d.byteOffset + d.byteLength)));
    this.wake?.();
  };

  constructor(private readonly device: HidDeviceLike) {
    device.addEventListener("inputreport", this.onReport);
  }

  /** The next input report, or null once `deadline` passes or the transport ends. */
  private async nextReport(deadline: number): Promise<Uint8Array | null> {
    while (this.queue.length === 0) {
      if (this.gone) return null;
      const left = deadline - Date.now();
      if (left <= 0) return null;
      await new Promise<void>((resolve) => {
        const timer = setTimeout(done, left);
        function done(): void {
          clearTimeout(timer);
          resolve();
        }
        this.wake = done;
      });
      this.wake = null;
    }
    return this.queue.shift() ?? null;
  }

  private async readReply(timeoutMs: number): Promise<Uint8Array> {
    const deadline = Date.now() + timeoutMs;
    const first = await this.nextReport(deadline);
    if (this.gone) throw this.gone;
    if (!first) throw timeoutError();
    const want = declaredLength(first);
    if (want <= first.length) return first.slice(0, Math.max(want, 0));
    const out = new Uint8Array(want);
    out.set(first);
    let have = first.length;
    while (have < want) {
      const r = await this.nextReport(deadline);
      if (this.gone) throw this.gone;
      if (!r) throw timeoutError();
      const take = Math.min(r.length, want - have);
      out.set(r.subarray(0, take), have);
      have += take;
    }
    return out;
  }

  async transact(frame: Uint8Array, timeoutMs: number): Promise<Uint8Array> {
    if (this.gone) throw this.gone;
    if (frame.length > REPORT_LEN) {
      throw transportError("usage", "The request does not fit in one HID report.", null);
    }
    const report = new Uint8Array(REPORT_LEN);
    report.set(frame);
    this.queue = [];
    try {
      await this.device.sendReport(0, report);
    } catch {
      this.end();
      throw disconnectedError();
    }
    return this.readReply(timeoutMs);
  }

  async read(timeoutMs: number): Promise<Uint8Array> {
    if (this.gone) throw this.gone;
    return this.readReply(timeoutMs);
  }

  private end(): void {
    if (this.gone) return;
    this.gone = disconnectedError();
    this.device.removeEventListener("inputreport", this.onReport);
    this.wake?.();
  }

  async close(): Promise<void> {
    if (this.gone) return;
    this.end();
    await this.device.close().catch(() => {});
  }
}
