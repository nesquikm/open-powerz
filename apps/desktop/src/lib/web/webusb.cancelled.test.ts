/**
 * FR 7T1YGJ: WebUSB recovery skips the reads it cancels itself.
 *
 * - AC-7T1YGJ.1: during recovery, an IN transfer that was pending before it and rejects
 *   is skipped as cancelled; the transport is not ended as disconnected.
 * - AC-7T1YGJ.2: during recovery, a halt clear that fails is logged and recovery goes on
 *   to the drain.
 * - AC-7T1YGJ.4: a real unplug (the browser's disconnect event) during or after recovery
 *   still ends the transport as disconnected.
 * - AC-7T1YGJ.5: the debug log adds the last 4 bytes of any IN frame longer than 8
 *   bytes, and the error name of a failed transfer or halt clear.
 *
 * The fake device's `clearHaltCancelsIn` ends the transferIn still waiting with an
 * AbortError when the IN halt is cleared, as Chrome did in web HIL run
 * WEB.hil2-20261108-2223 (W2 try 1). The driver half (AC-7T1YGJ.3) is in
 * `driver.recovery.test.ts`.
 */

import { afterEach, describe, expect, it, vi } from "vitest";
import type { UiError } from "../backend";
import { FakeUsb, FakeUsbDevice } from "./fakeDevices.test";
import { loadWasm } from "./testWasm.test";
import type { WebTransport } from "./transport";
import { openWebUsb } from "./webusb";

type UsbArg = Parameters<typeof openWebUsb>[0];
type UsbOptions = NonNullable<Parameters<typeof openWebUsb>[1]>;
type Recoverable = WebTransport & { recover: () => Promise<void> };

const asUsb = (d: FakeUsbDevice): UsbArg => d as unknown as UsbArg;

/** An ACCEPT echoing the request's id. */
const accept = (frame: Uint8Array) => new Uint8Array([0x05, frame[1] ?? 0, 0x00, 0x00]);

/** The 68-byte stale frame that answered the first CONNECT on the HIL run: it starts
 * `67 d6 a4 86 f5 28 d5 8f` and here ends `9a bc de f1`. */
const STALE_68 = (() => {
  const f = new Uint8Array(68);
  f.set([0x67, 0xd6, 0xa4, 0x86, 0xf5, 0x28, 0xd5, 0x8f]);
  for (let i = 8; i < 64; i++) f[i] = 0x33;
  f.set([0x9a, 0xbc, 0xde, 0xf1], 64);
  return f;
})();

const STALE_A = new Uint8Array([0x03, 0x2a, 0x00, 0x00]);

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});

async function open(
  dev: FakeUsbDevice,
  extra: Partial<Record<keyof UsbOptions, unknown>> = {},
): Promise<Recoverable> {
  return (await openWebUsb(asUsb(dev), {
    debug: false,
    ...extra,
  } as unknown as UsbOptions)) as Recoverable;
}

/** Reproduce the HIL run up to the recovery: the first CONNECT gets the stale 68-byte
 * frame, the re-read for its reply times out and its transferIn stays waiting. */
async function stuckAfterFirstConnect(t: Recoverable, dev: FakeUsbDevice): Promise<void> {
  const w = await loadWasm();
  let first = true;
  dev.respond = () => {
    if (!first) return null;
    first = false;
    return STALE_68;
  };
  const reply = await t.transact(w.connectRequest(1), 500);
  expect(reply.length).toBe(68);
  await expect(t.read(30)).rejects.toMatchObject({ kind: "timeout" });
  // The timed-out re-read is still queued on the pipe.
  expect(dev.waitingIn).toBeGreaterThanOrEqual(1);
}

/** Every console line written while the spies are on. */
function captureConsole(): string[] {
  const lines: string[] = [];
  for (const m of ["log", "debug", "info", "warn", "error"] as const) {
    vi.spyOn(console, m).mockImplementation((...args: unknown[]) => {
      lines.push(
        args
          .map((a) =>
            typeof a === "string" ? a : a instanceof Error ? `${a.name} ${a.message}` : String(a),
          )
          .join(" "),
      );
    });
  }
  return lines;
}

/** `line` lowercased with hex separators and 0x prefixes removed. */
const squash = (line: string) =>
  line
    .toLowerCase()
    .replace(/\b0x/g, "")
    .replace(/[\s:,]/g, "");

describe("a pending IN transfer cancelled by recovery (AC-7T1YGJ.1)", () => {
  it("is skipped: recover() resolves and the next CONNECT gets its reply", async () => {
    const w = await loadWasm();
    const dev = new FakeUsbDevice();
    dev.clearHaltCancelsIn = true;
    const t = await open(dev);
    await stuckAfterFirstConnect(t, dev);
    await expect(t.recover()).resolves.toBeUndefined();
    expect(dev.calls).toContain("clearHalt:in:6");
    dev.respond = accept;
    const reply = await t.transact(w.connectRequest(2), 500);
    expect(Array.from(reply)).toEqual([0x05, 0x02, 0x00, 0x00]);
    await t.close();
  });

  it("a timed-out CONNECT whose transfer the halt clear cancels: the transport lives on", async () => {
    const w = await loadWasm();
    const dev = new FakeUsbDevice(() => null);
    dev.clearHaltCancelsIn = true;
    const t = await open(dev);
    await expect(t.transact(w.connectRequest(1), 30)).rejects.toMatchObject({ kind: "timeout" });
    await t.recover();
    // Not ended as disconnected: reads still time out, they do not fail as unplugged.
    await expect(t.read(30)).rejects.toMatchObject({ kind: "timeout" });
    dev.respond = accept;
    const reply = await t.transact(w.connectRequest(2), 500);
    expect(reply[1]).toBe(0x02);
    await t.close();
  });

  it("stale data still on the pipe after the cancelled transfer is drained", async () => {
    const w = await loadWasm();
    const dev = new FakeUsbDevice(() => null);
    dev.clearHaltCancelsIn = true;
    const t = await open(dev);
    await expect(t.transact(w.connectRequest(1), 30)).rejects.toMatchObject({ kind: "timeout" });
    const seen = dev.inResults.length;
    const recovering = t.recover();
    // The late reply to the first CONNECT lands while recovery drains.
    dev.queueIn(new Uint8Array([0x05, 0x01, 0x00, 0x00]));
    await recovering;
    expect(dev.pendingIn).toBe(0);
    expect(dev.inResults.length).toBeGreaterThan(seen);
    dev.respond = accept;
    const reply = await t.transact(w.connectRequest(2), 500);
    expect(Array.from(reply)).toEqual([0x05, 0x02, 0x00, 0x00]);
    await t.close();
  });

  it("only during recovery: a transferIn that fails after it still ends the transport", async () => {
    const w = await loadWasm();
    const dev = new FakeUsbDevice(() => null);
    dev.clearHaltCancelsIn = true;
    const t = await open(dev);
    await expect(t.transact(w.connectRequest(1), 30)).rejects.toMatchObject({ kind: "timeout" });
    await t.recover();
    dev.inError = new DOMException("A transfer error has occurred.", "NetworkError");
    await expect(t.read(500)).rejects.toMatchObject({ kind: "disconnected" });
    await expect(t.transact(w.connectRequest(2), 500)).rejects.toMatchObject({
      kind: "disconnected",
    });
    await t.close();
  });
});

describe("a halt clear that fails during recovery (AC-7T1YGJ.2)", () => {
  it("recovery goes on to the drain and resolves", async () => {
    const w = await loadWasm();
    const dev = new FakeUsbDevice(() => null);
    const t = await open(dev);
    dev.clearHaltError = new DOMException("The clear halt failed.", "InvalidStateError");
    dev.queueIn(STALE_A);
    const reads = dev.inTransfers.length;
    await expect(t.recover()).resolves.toBeUndefined();
    // Both halts were tried, then the drain read the stale frame off the pipe.
    expect(dev.calls).toContain("clearHalt:out:5");
    expect(dev.calls).toContain("clearHalt:in:6");
    expect(dev.inTransfers.length).toBeGreaterThan(reads);
    expect(dev.pendingIn).toBe(0);
    expect(dev.inResults).toContainEqual(STALE_A);
    dev.clearHaltError = null;
    dev.respond = accept;
    const reply = await t.transact(w.connectRequest(2), 500);
    expect(Array.from(reply)).toEqual([0x05, 0x02, 0x00, 0x00]);
    await t.close();
  });

  it("logs the failed halt clear, then the drain", async () => {
    const lines = captureConsole();
    const dev = new FakeUsbDevice(() => null);
    const t = await open(dev, { debug: true });
    dev.clearHaltError = new DOMException("The clear halt failed.", "InvalidStateError");
    lines.length = 0;
    await t.recover();
    const failed = lines.findIndex((l) => /halt/i.test(l) && /InvalidStateError/.test(l));
    expect(failed, lines.join("\n")).toBeGreaterThanOrEqual(0);
    const drained = lines.findIndex((l, i) => i > failed && /drain/i.test(l));
    expect(drained, lines.join("\n")).toBeGreaterThan(failed);
    await t.close();
  });

  it("a cancelled pending transfer plus failing halt clears: recovery still resolves", async () => {
    const w = await loadWasm();
    const dev = new FakeUsbDevice();
    dev.clearHaltCancelsIn = true;
    const t = await open(dev);
    await stuckAfterFirstConnect(t, dev);
    dev.clearHaltError = new DOMException("The clear halt failed.", "NetworkError");
    await expect(t.recover()).resolves.toBeUndefined();
    dev.clearHaltError = null;
    dev.respond = accept;
    const reply = await t.transact(w.connectRequest(2), 500);
    expect(reply[1]).toBe(0x02);
    await t.close();
  });
});

/** A fake device whose IN halt clear waits until `release()` (cancelling the waiting
 * transferIn at once when `clearHaltCancelsIn` is set). */
class HeldHaltDevice extends FakeUsbDevice {
  private gate: (() => void) | null = null;
  holdIn = false;
  override async clearHalt(direction: "in" | "out", endpointNumber: number): Promise<void> {
    if (direction === "in" && this.holdIn) {
      this.calls.push(`clearHalt:${direction}:${endpointNumber}`);
      if (this.clearHaltCancelsIn) this.cancelWaitingIn();
      await new Promise<void>((resolve) => {
        this.gate = resolve;
      });
      return;
    }
    return super.clearHalt(direction, endpointNumber);
  }
  get holding(): boolean {
    return this.gate !== null;
  }
  release(): void {
    this.gate?.();
    this.gate = null;
  }
}

/** Let pending microtasks and 0 ms timers run. */
const tick = () => new Promise((r) => setTimeout(r, 0));

describe("a real unplug still ends the transport (AC-7T1YGJ.4)", () => {
  it("an unplug while the IN halt clear runs ends recovery and the transport as disconnected", async () => {
    const w = await loadWasm();
    const usb = new FakeUsb();
    const dev = new HeldHaltDevice(() => null);
    dev.clearHaltCancelsIn = true;
    const t = await open(dev, { usb });
    await expect(t.transact(w.connectRequest(1), 30)).rejects.toMatchObject({ kind: "timeout" });
    dev.holdIn = true;
    const recovering = t.recover().then(
      () => null,
      (e: unknown) => e as UiError,
    );
    for (let i = 0; i < 10 && !dev.holding; i++) await tick();
    expect(dev.holding).toBe(true);
    usb.unplug(dev);
    dev.release();
    expect(await recovering).toMatchObject({ kind: "disconnected" });
    dev.respond = accept;
    await expect(t.transact(w.connectRequest(2), 500)).rejects.toMatchObject({
      kind: "disconnected",
    });
    await expect(t.read(500)).rejects.toMatchObject({ kind: "disconnected" });
    await t.close();
  });

  it("an unplug while recovery drains ends it as disconnected", async () => {
    const w = await loadWasm();
    const usb = new FakeUsb();
    const dev = new FakeUsbDevice(() => null);
    dev.clearHaltCancelsIn = true;
    const t = await open(dev, { usb });
    await expect(t.transact(w.connectRequest(1), 30)).rejects.toMatchObject({ kind: "timeout" });
    const recovering = t.recover().then(
      () => null,
      (e: unknown) => e as UiError,
    );
    await tick();
    usb.unplug(dev);
    dev.cancelWaitingIn();
    expect(await recovering).toMatchObject({ kind: "disconnected" });
    await expect(t.read(500)).rejects.toMatchObject({ kind: "disconnected" });
    await t.close();
  });

  it("an unplug after a recovery that skipped a cancelled transfer ends the transport", async () => {
    const w = await loadWasm();
    const usb = new FakeUsb();
    const dev = new FakeUsbDevice(() => null);
    dev.clearHaltCancelsIn = true;
    const t = await open(dev, { usb });
    await expect(t.transact(w.connectRequest(1), 30)).rejects.toMatchObject({ kind: "timeout" });
    await t.recover();
    const started = Date.now();
    const pending = t.transact(w.connectRequest(2), 2_000);
    await tick();
    usb.unplug(dev);
    await expect(pending).rejects.toMatchObject({ kind: "disconnected" });
    expect(Date.now() - started).toBeLessThan(1_000);
    await expect(t.recover()).rejects.toMatchObject({ kind: "disconnected" });
    await t.close();
  });
});

describe("debug log detail (AC-7T1YGJ.5)", () => {
  it("an IN frame longer than 8 bytes also shows its last 4 bytes", async () => {
    const lines = captureConsole();
    const w = await loadWasm();
    const dev = new FakeUsbDevice(() => STALE_68);
    const t = await open(dev, { debug: true });
    await t.transact(w.connectRequest(1), 500);
    const line = lines.find((l) => squash(l).includes("67d6a486f528d58f"));
    expect(line, lines.join("\n")).toBeDefined();
    expect(line).toMatch(/\bin\b/i);
    expect(line).toMatch(/\b68\b/);
    expect(squash(line ?? "")).toContain("9abcdef1");
    // Only the first 8 and the last 4: none of the 56 bytes between them.
    expect(squash(line ?? "")).not.toContain("33");
    await t.close();
  });

  it("a 10-byte IN frame shows its first 8 and last 4 bytes", async () => {
    const lines = captureConsole();
    const frame = new Uint8Array([0x41, 0x01, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff, 0x11, 0x22]);
    const dev = new FakeUsbDevice(() => frame);
    const t = await open(dev, { debug: true });
    await t.transact(new Uint8Array([0x0c, 0x01, 0x02, 0x00]), 500);
    const line = lines.find((l) => squash(l).includes("4101aabbccddeeff"));
    expect(line, lines.join("\n")).toBeDefined();
    const tail = squash(line ?? "").split("4101aabbccddeeff")[1] ?? "";
    expect(tail).toContain("eeff1122");
    await t.close();
  });

  it("an IN frame of 8 bytes or fewer is not repeated", async () => {
    const lines = captureConsole();
    const frame = new Uint8Array([0x41, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07]);
    const dev = new FakeUsbDevice(() => frame);
    const t = await open(dev, { debug: true });
    await t.transact(new Uint8Array([0x0c, 0x01, 0x02, 0x00]), 500);
    const line = lines.find((l) => /\bin\b/i.test(l) && squash(l).includes("4101020304050607"));
    expect(line, lines.join("\n")).toBeDefined();
    expect(squash(line ?? "").split("04050607").length - 1).toBe(1);
    await t.close();
  });

  it("a failed IN transfer logs the error name", async () => {
    const lines = captureConsole();
    const dev = new FakeUsbDevice(() => null);
    const t = await open(dev, { debug: true });
    dev.inError = new DOMException("A transfer error has occurred.", "NetworkError");
    await expect(t.read(500)).rejects.toMatchObject({ kind: "disconnected" });
    expect(
      lines.some((l) => /NetworkError/.test(l)),
      lines.join("\n"),
    ).toBe(true);
    await t.close();
  });

  it("a failed OUT transfer logs the error name", async () => {
    const lines = captureConsole();
    const w = await loadWasm();
    const dev = new FakeUsbDevice(() => null);
    const t = await open(dev, { debug: true });
    dev.transferOut = () =>
      Promise.reject(new DOMException("A transfer error has occurred.", "NetworkError"));
    await expect(t.transact(w.connectRequest(1), 500)).rejects.toMatchObject({
      kind: "disconnected",
    });
    expect(
      lines.some((l) => /out: transfer failed \(NetworkError\)/.test(l)),
      lines.join("\n"),
    ).toBe(true);
    await t.close();
  });

  it("a transfer cancelled during recovery logs the error name", async () => {
    const lines = captureConsole();
    const w = await loadWasm();
    const dev = new FakeUsbDevice(() => null);
    dev.clearHaltCancelsIn = true;
    const t = await open(dev, { debug: true });
    await expect(t.transact(w.connectRequest(1), 30)).rejects.toMatchObject({ kind: "timeout" });
    lines.length = 0;
    await t.recover();
    expect(
      lines.some((l) => /AbortError/.test(l)),
      lines.join("\n"),
    ).toBe(true);
    await t.close();
  });

  it("a failed halt clear logs the error name", async () => {
    const lines = captureConsole();
    const dev = new FakeUsbDevice(() => null);
    const t = await open(dev, { debug: true });
    dev.clearHaltError = new DOMException("The clear halt failed.", "NotFoundError");
    lines.length = 0;
    await t.recover().catch(() => {});
    expect(
      lines.some((l) => /halt/i.test(l) && /NotFoundError/.test(l)),
      lines.join("\n"),
    ).toBe(true);
    await t.close();
  });
});
