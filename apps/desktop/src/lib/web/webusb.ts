/**
 * FR QFTCWZ: the WebUSB transport. Opens a VID 0x5FC9 meter, picks the class-0xFF
 * interface with one bulk IN and one bulk OUT endpoint (by class, never by number),
 * and frames requests like the native bulk transport.
 */

import type { UiError } from "../backend";
import {
  disconnectedError,
  timeoutError,
  transportError,
  withTimeout,
  type WebTransport,
} from "./transport";

export const POWERZ_VENDOR_ID = 0x5fc9;

const VENDOR_CLASS = 0xff;
const PACKET = 64;
const IN_LENGTH = 4096;
/** How long one drain read waits for stale data at open (as the native bulk open). */
const DRAIN_TIMEOUT_MS = 50;

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
  transferIn(
    endpoint: number,
    length: number,
  ): Promise<{ status?: string; data?: DataView | null }>;
  clearHalt(direction: "in" | "out", endpointNumber: number): Promise<void>;
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

/** Timeouts raised by a STALL: the transfer is spent, so it never goes back in line. */
const stallErrors = new WeakSet<object>();

/** A STALL fails as `timeout` (so connect recovery can run), never as an empty reply. */
function stallError(): UiError {
  const e = timeoutError();
  stallErrors.add(e);
  return e;
}

function isStall(e: unknown): boolean {
  return typeof e === "object" && e !== null && stallErrors.has(e);
}

export interface WebUsbOptions {
  usb?: UsbLike;
  /** Log frames and lifecycle to the console; read from the page when left out. */
  debug?: boolean;
}

/** The localStorage key that turns the debug log on when set to "1". */
export const WEB_DEBUG_STORAGE_KEY = "open-powerz.debug";

/** The debug log is on with a `debug` query parameter or the localStorage flag set to "1". */
export function webDebugEnabled(
  search: string,
  storage: { getItem(k: string): string | null } | null,
): boolean {
  try {
    if (new URLSearchParams(search).has("debug")) return true;
  } catch {
    // An unparsable query string leaves the flag to storage.
  }
  try {
    return storage?.getItem(WEB_DEBUG_STORAGE_KEY) === "1";
  } catch {
    return false;
  }
}

/** `get()`, or null when it throws or yields nothing. */
function orNull<T>(get: () => T | undefined | null): T | null {
  try {
    return get() ?? null;
  } catch {
    return null;
  }
}

/** Read the debug flag from the page; `location` and `localStorage` may be absent or throw. */
function pageDebugEnabled(): boolean {
  const search = orNull(() => globalThis.location?.search) ?? "";
  return webDebugEnabled(
    search,
    orNull(() => globalThis.localStorage),
  );
}

type DebugLog = (message: string) => void;

function debugLog(enabled: boolean): DebugLog {
  return enabled ? (message) => console.debug(`[webusb] ${message}`) : () => {};
}

/** A frame as `<dir> <length> bytes: <first 8 bytes in hex>`. */
function describeFrame(dir: "in" | "out", data: Uint8Array): string {
  const hex = Array.from(data.subarray(0, 8), (b) => b.toString(16).padStart(2, "0")).join(" ");
  return `${dir} ${data.length} bytes: ${hex}`;
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
  const log = debugLog(options.debug ?? pageDebugEnabled());
  log(`open: interface ${pick.iface}, bulk in ${pick.inEp}, bulk out ${pick.outEp}`);
  const transport = new WebUsbTransport(
    device,
    pick.iface,
    pick.inEp,
    pick.outEp,
    options.usb,
    log,
  );
  await transport.drainStale();
  return transport;
}

class WebUsbTransport implements WebTransport {
  private gone: UiError | null = null;
  /** Set by the first `close()`: release + close run at most once, so a late second
   * call never touches a device a newer session has reopened and claimed. */
  private closed = false;
  /** IN transfers issued but not yet received by a caller, oldest first. */
  private pendingIn: Promise<Uint8Array>[] = [];
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
    private readonly log: DebugLog = () => {},
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

  /** A failed USB call means the meter is gone: end the transport and fail as disconnected. */
  private lost(): never {
    this.end();
    throw disconnectedError();
  }

  private startIn(): Promise<Uint8Array> {
    const inn = this.device.transferIn(this.inEp, IN_LENGTH).then(
      async (r) => {
        if (r.status === "stall") {
          await this.clearHalt("in", this.inEp);
          throw stallError();
        }
        const d = r.data;
        const data = d
          ? new Uint8Array(d.buffer.slice(d.byteOffset, d.byteOffset + d.byteLength))
          : new Uint8Array();
        this.log(describeFrame("in", data));
        return data;
      },
      (e: unknown) => {
        if (isStall(e)) throw e;
        this.lost();
      },
    );
    inn.catch(() => {});
    return inn;
  }

  /** Clear a halted endpoint after a STALL; a failing clear means the meter is gone. */
  private async clearHalt(direction: "in" | "out", endpoint: number): Promise<void> {
    try {
      await this.device.clearHalt(direction, endpoint);
    } catch {
      this.lost();
    }
  }

  /**
   * Issue one more IN transfer and hand out the oldest unreceived one. A transferIn
   * left over from a timed-out call (a drain read, a late reply) is still the next
   * reply; the fresh transfer still surfaces a failing pipe at once.
   */
  private nextIn(): Promise<Uint8Array> {
    this.pendingIn.push(this.startIn());
    return this.pendingIn.shift() as Promise<Uint8Array>;
  }

  /** Await `work` (which yields IN transfer `inn()`); a timed-out transfer goes back first in line. */
  private async receive(
    work: Promise<Uint8Array>,
    inn: () => Promise<Uint8Array> | null,
    timeoutMs: number,
  ): Promise<Uint8Array> {
    try {
      return await this.guard(work, timeoutMs);
    } catch (e) {
      const left = inn();
      const requeue = (e as UiError | null)?.kind === "timeout" && !isStall(e);
      if (left && requeue) this.pendingIn.unshift(left);
      throw e;
    }
  }

  private guard<T>(work: Promise<T>, timeoutMs: number): Promise<T> {
    return withTimeout(Promise.race([work, this.aborted]), timeoutMs);
  }

  /** Discard every IN transfer already waiting: read until one read times out. */
  async drainStale(): Promise<number> {
    let discarded = 0;
    for (;;) {
      try {
        await this.read(DRAIN_TIMEOUT_MS);
        discarded += 1;
      } catch (e) {
        if ((e as UiError | null)?.kind === "timeout") {
          this.log(`drain: discarded ${discarded} stale frame(s)`);
          return discarded;
        }
        throw e;
      }
    }
  }

  /** As the native bulk `recover`: clear the halt on bulk OUT and IN, then drain. */
  async recover(): Promise<void> {
    if (this.gone) throw this.gone;
    this.log("recover: clearing halt on bulk out and in");
    await this.clearHalt("out", this.outEp);
    await this.clearHalt("in", this.inEp);
    await this.drainStale();
  }

  async transact(frame: Uint8Array, timeoutMs: number): Promise<Uint8Array> {
    if (this.gone) throw this.gone;
    const out = async (data: Uint8Array<ArrayBuffer>): Promise<boolean> => {
      let r: unknown;
      this.log(describeFrame("out", data));
      try {
        r = await this.device.transferOut(this.outEp, data);
      } catch {
        this.lost();
      }
      return (r as { status?: string } | null)?.status === "stall";
    };
    const send = async (): Promise<void> => {
      let stalled = await out(new Uint8Array(frame));
      if (!stalled && frame.length > 0 && frame.length % PACKET === 0) {
        stalled = await out(new Uint8Array());
      }
      if (stalled) {
        await this.clearHalt("out", this.outEp);
        throw stallError();
      }
    };
    let inn: Promise<Uint8Array> | null = null;
    let late = false;
    const work = send().then(() => {
      inn = this.nextIn();
      // Timed out while sending: nobody will receive this transfer, keep it in line.
      if (late) this.pendingIn.unshift(inn);
      return inn;
    });
    return this.receive(
      work,
      () => {
        late = inn === null;
        return inn;
      },
      timeoutMs,
    );
  }

  async read(timeoutMs: number): Promise<Uint8Array> {
    if (this.gone) throw this.gone;
    const inn = this.nextIn();
    return this.receive(inn, () => inn, timeoutMs);
  }

  async close(): Promise<void> {
    if (this.closed) return;
    this.closed = true;
    if (!this.gone) {
      this.log("close");
      this.end();
    }
    await this.device.releaseInterface(this.iface).catch(() => {});
    await this.device.close().catch(() => {});
  }
}
