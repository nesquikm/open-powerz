/**
 * FR G30K4R: the WebUSB transport frees the meter on close.
 *
 * - AC-G30K4R.2: close releases the interface and closes the device even when the
 *   transport already ended on a disconnect or a failed transfer; a release or close
 *   that throws is ignored.
 * - AC-G30K4R.3: after close, a transfer still pending from the failed session can no
 *   longer deliver a reply to a later session on the same device.
 */

import { afterEach, describe, expect, it, vi } from "vitest";
import { FakeUsb, FakeUsbDevice } from "./fakeDevices.test";
import { loadWasm } from "./testWasm.test";
import { openWebUsb } from "./webusb";

type UsbArg = Parameters<typeof openWebUsb>[0];
type UsbOptions = NonNullable<Parameters<typeof openWebUsb>[1]>;

const asUsb = (d: FakeUsbDevice): UsbArg => d as unknown as UsbArg;
const opts = (o: Record<string, unknown>): UsbOptions => o as unknown as UsbOptions;

/** An ACCEPT echoing the request's id. */
const accept = (frame: Uint8Array) => new Uint8Array([0x05, frame[1] ?? 0, 0x00, 0x00]);

/** The vendor bulk interface of `meterConfiguration()`. */
const VENDOR_IFACE = 4;

const claimed = (dev: FakeUsbDevice): boolean =>
  dev.configuration?.interfaces.find((i) => i.interfaceNumber === VENDOR_IFACE)?.claimed === true;

afterEach(() => {
  vi.useRealTimers();
});

describe("close frees the meter after the transport ended (AC-G30K4R.2)", () => {
  it("after a disconnect event, close releases the interface and closes the device", async () => {
    const usb = new FakeUsb();
    const dev = new FakeUsbDevice(accept);
    const t = await openWebUsb(asUsb(dev), opts({ debug: false, usb }));
    expect(claimed(dev)).toBe(true);
    usb.unplug(dev);
    const mark = dev.calls.length;
    await t.close();
    const after = dev.calls.slice(mark);
    expect(after).toContain(`releaseInterface:${VENDOR_IFACE}`);
    expect(after).toContain("close");
    expect(after.indexOf("close")).toBeGreaterThan(
      after.indexOf(`releaseInterface:${VENDOR_IFACE}`),
    );
    expect(claimed(dev)).toBe(false);
    expect(dev.opened).toBe(false);
  });

  it("after a failed transfer, close releases the interface and closes the device", async () => {
    const w = await loadWasm();
    const dev = new FakeUsbDevice(accept);
    const t = await openWebUsb(asUsb(dev), opts({ debug: false }));
    dev.inError = new DOMException("The device was disconnected.", "NetworkError");
    await expect(t.transact(w.connectRequest(1), 500)).rejects.toMatchObject({
      kind: "disconnected",
    });
    const mark = dev.calls.length;
    await t.close();
    const after = dev.calls.slice(mark);
    expect(after).toContain(`releaseInterface:${VENDOR_IFACE}`);
    expect(after).toContain("close");
    expect(claimed(dev)).toBe(false);
    expect(dev.opened).toBe(false);
  });

  it("a release that throws is ignored and the device is still closed", async () => {
    const dev = new FakeUsbDevice(accept);
    const t = await openWebUsb(asUsb(dev), opts({ debug: false }));
    dev.releaseError = new DOMException("gone", "NotFoundError");
    await expect(t.close()).resolves.toBeUndefined();
    expect(dev.calls).toContain(`releaseInterface:${VENDOR_IFACE}`);
    expect(dev.calls).toContain("close");
    expect(dev.opened).toBe(false);
  });

  it("a release and a close that throw after a disconnect are both tried and ignored", async () => {
    const usb = new FakeUsb();
    const dev = new FakeUsbDevice(accept);
    const t = await openWebUsb(asUsb(dev), opts({ debug: false, usb }));
    usb.unplug(dev);
    dev.releaseError = new DOMException("gone", "NotFoundError");
    dev.closeError = new DOMException("gone", "NotFoundError");
    const mark = dev.calls.length;
    await expect(t.close()).resolves.toBeUndefined();
    const after = dev.calls.slice(mark);
    expect(after).toContain(`releaseInterface:${VENDOR_IFACE}`);
    expect(after).toContain("close");
  });
});

describe("a closed session's pending transfer (AC-G30K4R.3)", () => {
  it("after a timed-out request, nothing is left waiting on bulk IN once closed", async () => {
    const w = await loadWasm();
    const dev = new FakeUsbDevice(() => null);
    const t = await openWebUsb(asUsb(dev), opts({ debug: false }));
    await expect(t.transact(w.connectRequest(1), 30)).rejects.toMatchObject({ kind: "timeout" });
    expect(dev.waitingIn).toBeGreaterThan(0);
    await t.close();
    expect(dev.waitingIn).toBe(0);
  });

  it("after a disconnect, the next session on the same device gets its own reply", async () => {
    const w = await loadWasm();
    const usb = new FakeUsb();
    const dev = new FakeUsbDevice(() => null);
    const first = await openWebUsb(asUsb(dev), opts({ debug: false, usb }));
    // The first session's CONNECT gets no answer; its IN transfer stays submitted.
    await expect(first.transact(w.connectRequest(1), 30)).rejects.toMatchObject({
      kind: "timeout",
    });
    usb.unplug(dev);
    await first.close();
    expect(dev.waitingIn).toBe(0);

    dev.respond = accept;
    const second = await openWebUsb(asUsb(dev), opts({ debug: false, usb }));
    const reply = await second.transact(w.connectRequest(2), 500);
    expect(Array.from(reply)).toEqual([0x05, 0x02, 0x00, 0x00]);
    await second.close();
  });

  it("a reply arriving after close is not handed to the closed transport's callers", async () => {
    const w = await loadWasm();
    const dev = new FakeUsbDevice(() => null);
    const t = await openWebUsb(asUsb(dev), opts({ debug: false }));
    await expect(t.transact(w.connectRequest(1), 30)).rejects.toMatchObject({ kind: "timeout" });
    await t.close();
    dev.queueIn(new Uint8Array([0x05, 0x01, 0x00, 0x00]));
    await expect(t.read(30)).rejects.toMatchObject({ kind: "disconnected" });
    // The late reply stays on the pipe for whoever opens the device next.
    expect(dev.pendingIn).toBe(1);
  });
});
