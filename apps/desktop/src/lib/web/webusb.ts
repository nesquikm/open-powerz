/**
 * FR QFTCWZ: the WebUSB transport. Opens a VID 0x5FC9 meter, picks the class-0xFF
 * interface with one bulk IN and one bulk OUT endpoint (by class, never by number),
 * and frames requests like the native bulk transport.
 */

import type { UiError } from "../backend";
import { disconnectedError, transportError, withTimeout, type WebTransport } from "./transport";

export const POWERZ_VENDOR_ID = 0x5fc9;

const VENDOR_CLASS = 0xff;
const PACKET = 64;
const IN_LENGTH = 4096;

/** The parts of the WebUSB `USBDevice` the transport uses. */
export interface UsbDeviceLike {
  vendorId: number;
  configuration: UsbConfigurationLike | null;
  configurations: UsbConfigurationLike[];
  open(): Promise<void>;
  close(): Promise<void>;
  selectConfiguration(value: number): Promise<void>;
  claimInterface(number: number): Promise<void>;
  releaseInterface(number: number): Promise<void>;
  transferOut(endpoint: number, data: BufferSource): Promise<unknown>;
  transferIn(endpoint: number, length: number): Promise<{ data?: DataView | null }>;
}

interface UsbConfigurationLike {
  configurationValue: number;
  interfaces: {
    interfaceNumber: number;
    alternate: {
      interfaceClass: number;
      endpoints: { endpointNumber: number; direction: "in" | "out"; type: string }[];
    };
  }[];
}

/** The parts of `navigator.usb` the transport uses: the disconnect event. */
export interface UsbLike {
  addEventListener(type: "disconnect", fn: (event: { device: unknown }) => void): void;
  removeEventListener(type: "disconnect", fn: (event: { device: unknown }) => void): void;
}

export interface WebUsbOptions {
  usb?: UsbLike;
}

function pickBulk(
  config: UsbConfigurationLike,
): { iface: number; inEp: number; outEp: number } | null {
  for (const i of config.interfaces) {
    if (i.alternate.interfaceClass !== VENDOR_CLASS) continue;
    const bulk = i.alternate.endpoints.filter((e) => e.type === "bulk");
    const inEp = bulk.find((e) => e.direction === "in");
    const outEp = bulk.find((e) => e.direction === "out");
    if (inEp && outEp)
      return { iface: i.interfaceNumber, inEp: inEp.endpointNumber, outEp: outEp.endpointNumber };
  }
  return null;
}

/** Open `device` as a meter over its vendor bulk interface. */
export async function openWebUsb(
  device: UsbDeviceLike,
  options: WebUsbOptions = {},
): Promise<WebTransport> {
  if (device.vendorId !== POWERZ_VENDOR_ID) {
    throw transportError("usage", "This is not a POWER-Z meter.", "Pick the KM003C or KM002C.");
  }
  await device.open();
  const value = device.configurations[0]?.configurationValue ?? 1;
  if (device.configuration?.configurationValue !== value) await device.selectConfiguration(value);
  const config = device.configuration ?? device.configurations[0];
  const pick = config ? pickBulk(config) : null;
  if (!pick) {
    await device.close().catch(() => {});
    throw transportError("protocol", "The meter has no vendor bulk interface.", null);
  }
  try {
    await device.claimInterface(pick.iface);
  } catch {
    await device.close().catch(() => {});
    throw transportError(
      "device_busy",
      "Another program is using the meter.",
      "Close other apps that use the meter and connect again.",
    );
  }
  return new WebUsbTransport(device, pick.iface, pick.inEp, pick.outEp, options.usb);
}

class WebUsbTransport implements WebTransport {
  private gone: UiError | null = null;
  private inflight: Promise<Uint8Array> | null = null;
  private abort: ((e: UiError) => void) | null = null;
  private readonly aborted: Promise<never>;
  private readonly onDisconnect = (event: { device: unknown }): void => {
    if (event.device === this.device) this.end();
  };

  constructor(
    private readonly device: UsbDeviceLike,
    private readonly iface: number,
    private readonly inEp: number,
    private readonly outEp: number,
    private readonly usb?: UsbLike,
  ) {
    this.aborted = new Promise<never>((_, reject) => {
      this.abort = reject;
    });
    this.aborted.catch(() => {});
    usb?.addEventListener("disconnect", this.onDisconnect);
  }

  private end(): void {
    if (this.gone) return;
    this.gone = disconnectedError();
    this.abort?.(this.gone);
    this.usb?.removeEventListener("disconnect", this.onDisconnect);
  }

  private nextIn(): Promise<Uint8Array> {
    // A transferIn left over from a timed-out call is still the next reply.
    if (!this.inflight) {
      this.inflight = this.device.transferIn(this.inEp, IN_LENGTH).then(
        (r) => {
          this.inflight = null;
          const d = r.data;
          return d
            ? new Uint8Array(d.buffer.slice(d.byteOffset, d.byteOffset + d.byteLength))
            : new Uint8Array();
        },
        () => {
          this.inflight = null;
          this.end();
          throw disconnectedError();
        },
      );
    }
    return this.inflight;
  }

  private guard<T>(work: Promise<T>, timeoutMs: number): Promise<T> {
    return withTimeout(Promise.race([work, this.aborted]), timeoutMs);
  }

  async transact(frame: Uint8Array, timeoutMs: number): Promise<Uint8Array> {
    if (this.gone) throw this.gone;
    const send = async (): Promise<void> => {
      try {
        await this.device.transferOut(this.outEp, new Uint8Array(frame));
        if (frame.length > 0 && frame.length % PACKET === 0) {
          await this.device.transferOut(this.outEp, new Uint8Array());
        }
      } catch {
        this.end();
        throw disconnectedError();
      }
    };
    return this.guard(
      send().then(() => this.nextIn()),
      timeoutMs,
    );
  }

  async read(timeoutMs: number): Promise<Uint8Array> {
    if (this.gone) throw this.gone;
    return this.guard(this.nextIn(), timeoutMs);
  }

  async close(): Promise<void> {
    if (this.gone) return;
    this.end();
    await this.device.releaseInterface(this.iface).catch(() => {});
    await this.device.close().catch(() => {});
  }
}
