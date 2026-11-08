/**
 * FR QFTCWZ, AC-QFTCWZ.2 and the WebUSB half of AC-QFTCWZ.6.
 *
 * The WebUSB transport opens a VID 0x5FC9 device, selects the configuration, picks the
 * class-0xFF interface with one bulk IN and one bulk OUT endpoint (never by number),
 * claims it and frames requests like the native bulk transport: one OUT transfer, a
 * zero-length packet after a non-empty multiple of 64 bytes, one IN transfer of up to
 * 4096 bytes. Transfer errors and the USB disconnect event end it as `disconnected`,
 * a reply that never comes as `timeout`.
 */

import { describe, expect, it } from "vitest";
import { FakeUsb, FakeUsbDevice, meterConfiguration } from "./fakeDevices.test";
import { loadWasm } from "./testWasm.test";
import { POWERZ_VENDOR_ID, openWebUsb } from "./webusb";

type UsbArg = Parameters<typeof openWebUsb>[0];
type UsbOptions = NonNullable<Parameters<typeof openWebUsb>[1]>;

function asUsb(d: FakeUsbDevice): UsbArg {
  return d as unknown as UsbArg;
}

async function demoDevice(): Promise<FakeUsbDevice> {
  const w = await loadWasm();
  const meter = new w.DemoMeter("load-5v-1a");
  return new FakeUsbDevice((frame) => {
    const reply = meter.transact(frame);
    return reply.length ? reply : null;
  });
}

describe("WebUSB transport: interface selection (AC-QFTCWZ.2)", () => {
  it("uses vendor ID 0x5FC9", () => {
    expect(POWERZ_VENDOR_ID).toBe(0x5fc9);
  });

  it("opens, selects the configuration and claims the class-0xFF bulk interface", async () => {
    const dev = await demoDevice();
    const t = await openWebUsb(asUsb(dev));
    expect(dev.calls.slice(0, 2)).toEqual(["open", "selectConfiguration:1"]);
    // Interface 1 is class 0xFF too but has no bulk pair; interface 3 has a bulk pair
    // but is CDC data. Only interface 4 qualifies.
    expect(dev.calls.filter((c) => c.startsWith("claimInterface"))).toEqual(["claimInterface:4"]);
    await t.close();
  });

  it("picks by class even when the vendor interface has another number", async () => {
    const config = meterConfiguration();
    const vendor = config.interfaces.find((i) => i.interfaceNumber === 4);
    if (!vendor) throw new Error("fixture");
    vendor.interfaceNumber = 0;
    config.interfaces[0]!.interfaceNumber = 4; // the HID interface now holds number 4
    const dev = new FakeUsbDevice(() => null, config);
    await openWebUsb(asUsb(dev));
    expect(dev.calls.filter((c) => c.startsWith("claimInterface"))).toEqual(["claimInterface:0"]);
  });

  it("refuses a device with no class-0xFF bulk interface without claiming anything", async () => {
    const config = meterConfiguration();
    config.interfaces = config.interfaces.filter((i) => i.interfaceNumber !== 4);
    const dev = new FakeUsbDevice(() => null, config);
    await expect(openWebUsb(asUsb(dev))).rejects.toHaveProperty("kind");
    expect(dev.calls.some((c) => c.startsWith("claimInterface"))).toBe(false);
  });

  it("refuses a device with another vendor ID", async () => {
    const dev = new FakeUsbDevice();
    dev.vendorId = 0x1234;
    await expect(openWebUsb(asUsb(dev))).rejects.toHaveProperty("kind");
    expect(dev.calls).not.toContain("claimInterface:4");
  });

  it("reports a claim failure as device_busy", async () => {
    const dev = new FakeUsbDevice();
    dev.claimError = new DOMException("Unable to claim interface.", "NetworkError");
    await expect(openWebUsb(asUsb(dev))).rejects.toMatchObject({ kind: "device_busy" });
  });
});

describe("WebUSB transport: framing (AC-QFTCWZ.2)", () => {
  it("sends one OUT transfer on the bulk OUT endpoint and reads one IN transfer", async () => {
    const w = await loadWasm();
    const dev = await demoDevice();
    const t = await openWebUsb(asUsb(dev));
    const reply = await t.transact(w.connectRequest(1), 500);
    expect(() => w.decodeAccept(reply)).not.toThrow();
    expect(reply[0]).toBe(0x05);
    expect(dev.outTransfers).toHaveLength(1);
    expect(dev.outTransfers[0]?.endpoint).toBe(5);
    expect(Array.from(dev.outTransfers[0]?.data ?? [])).toEqual(Array.from(w.connectRequest(1)));
    expect(dev.inTransfers.length).toBeGreaterThanOrEqual(1);
    expect(dev.inTransfers.every((i) => i.endpoint === 6 && i.length === 4096)).toBe(true);
  });

  it("follows a 64-byte write with a zero-length packet", async () => {
    const dev = new FakeUsbDevice(() => new Uint8Array([0x05, 0x01, 0x00, 0x00]));
    const t = await openWebUsb(asUsb(dev));
    await t.transact(new Uint8Array(64).fill(0x0c), 500);
    expect(dev.outTransfers.map((o) => o.data.length)).toEqual([64, 0]);
    expect(dev.outTransfers.every((o) => o.endpoint === 5)).toBe(true);
  });

  it("does not add a zero-length packet after a short write", async () => {
    const dev = new FakeUsbDevice(() => new Uint8Array([0x05, 0x01, 0x00, 0x00]));
    const t = await openWebUsb(asUsb(dev));
    await t.transact(new Uint8Array(36).fill(0x44), 500);
    expect(dev.outTransfers.map((o) => o.data.length)).toEqual([36]);
  });

  it("read() returns the next IN transfer without writing", async () => {
    const dev = new FakeUsbDevice(() => null);
    const t = await openWebUsb(asUsb(dev));
    // Prime a pending reply through the responder of a write the transport does not see.
    dev.respond = () => new Uint8Array([0x41, 0x02, 0x00, 0x00]);
    await dev.transferOut(5, new Uint8Array([1]));
    const before = dev.outTransfers.length;
    const got = await t.read(500);
    expect(Array.from(got)).toEqual([0x41, 0x02, 0x00, 0x00]);
    expect(dev.outTransfers.length).toBe(before);
  });

  it("close() releases the interface and closes the device", async () => {
    const dev = await demoDevice();
    const t = await openWebUsb(asUsb(dev));
    await t.close();
    expect(dev.calls).toContain("close");
    expect(dev.opened).toBe(false);
  });
});

describe("WebUSB transport: errors (AC-QFTCWZ.6)", () => {
  it("a reply that never comes is a timeout", async () => {
    const dev = new FakeUsbDevice(() => null);
    const t = await openWebUsb(asUsb(dev));
    const started = Date.now();
    await expect(t.transact(new Uint8Array([0x02, 0x01, 0x00, 0x00]), 40)).rejects.toMatchObject({
      kind: "timeout",
    });
    expect(Date.now() - started).toBeLessThan(1000);
  });

  it("a failed transfer is disconnected", async () => {
    const dev = new FakeUsbDevice(() => null);
    const t = await openWebUsb(asUsb(dev));
    dev.inError = new DOMException("A transfer error has occurred.", "NetworkError");
    await expect(t.transact(new Uint8Array([0x02, 0x01, 0x00, 0x00]), 500)).rejects.toMatchObject({
      kind: "disconnected",
    });
  });

  it("the USB disconnect event ends a pending transfer and later ones as disconnected", async () => {
    const usb = new FakeUsb();
    const dev = new FakeUsbDevice(() => null);
    const t = await openWebUsb(asUsb(dev), { usb } as unknown as UsbOptions);
    const pending = t.transact(new Uint8Array([0x02, 0x01, 0x00, 0x00]), 5000);
    usb.unplug(dev);
    await expect(pending).rejects.toMatchObject({ kind: "disconnected" });
    await expect(t.transact(new Uint8Array([0x02, 0x02, 0x00, 0x00]), 500)).rejects.toMatchObject({
      kind: "disconnected",
    });
  });

  it("a disconnect event for another device changes nothing", async () => {
    const usb = new FakeUsb();
    const dev = new FakeUsbDevice(() => new Uint8Array([0x05, 0x01, 0x00, 0x00]));
    const t = await openWebUsb(asUsb(dev), { usb } as unknown as UsbOptions);
    usb.unplug(new FakeUsbDevice());
    const reply = await t.transact(new Uint8Array([0x02, 0x01, 0x00, 0x00]), 500);
    expect(reply[0]).toBe(0x05);
  });
});
