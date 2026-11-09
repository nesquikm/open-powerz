/**
 * FR QFTCWZ, AC-QFTCWZ.3: the WebHID transport, ADC snapshots only (docs/protocol.md
 * "HID limits", [device] firmware 2.0.6: a reply over 64 bytes hangs and reboots the
 * meter, control commands get no reply). A guard lets through GET_DATA with attribute
 * mask 0x001 and refuses every other frame before anything is sent. The request goes
 * out as one 64-byte output report (report ID 0, the frame zero-padded); each 64-byte
 * input report is one whole reply.
 */

import type { UiError } from "../backend";
import {
  disconnectedError,
  isNotFoundError,
  timeoutError,
  transportError,
  type WebTransport,
} from "./transport";

const REPORT_LEN = 64;
const HEADER_LEN = 4;
/** GET_DATA message type [static]. */
const GET_DATA = 0x0c;
/** ATT_ADC, the only attribute HID may ask for [device]. */
const ATT_ADC = 0x001;

/** Whether `frame` is a plain GET_DATA for the ADC snapshot (any transaction id): the
 * one request HID carries. */
function isAdcSnapshotRequest(frame: Uint8Array): boolean {
  if (frame.length !== HEADER_LEN) return false;
  const typeByte = frame[0] ?? 0;
  // Bits 16..31 of the header: a reserved bit, then the 15-bit attribute mask.
  const high = (frame[2] ?? 0) | ((frame[3] ?? 0) << 8);
  return typeByte === GET_DATA && high === ATT_ADC << 1;
}

/** The parts of the WebHID `HIDDevice` the transport uses. */
export interface HidDeviceLike {
  opened: boolean;
  open(): Promise<void>;
  close(): Promise<void>;
  sendReport(reportId: number, data: BufferSource): Promise<void>;
  addEventListener(type: "inputreport", fn: (event: { data: DataView }) => void): void;
  removeEventListener(type: "inputreport", fn: (event: { data: DataView }) => void): void;
}

/** Open `device` as a meter over HID. */
export async function openWebHid(device: HidDeviceLike): Promise<WebTransport> {
  if (!device.opened) {
    try {
      await device.open();
    } catch (e) {
      if (isNotFoundError(e)) throw disconnectedError();
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

  /** The next input report as the whole reply (no multi-report assembly). */
  private async readReply(timeoutMs: number): Promise<Uint8Array> {
    const report = await this.nextReport(Date.now() + timeoutMs);
    if (this.gone) throw this.gone;
    if (!report) throw timeoutError();
    return report;
  }

  async transact(frame: Uint8Array, timeoutMs: number): Promise<Uint8Array> {
    if (this.gone) throw this.gone;
    if (!isAdcSnapshotRequest(frame)) {
      throw transportError(
        "usage",
        "Over HID the meter only answers basic readings (ADC snapshots).",
        null,
      );
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
