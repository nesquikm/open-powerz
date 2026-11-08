/**
 * FR QFTCWZ, AC-QFTCWZ.1: one `WebTransport` interface (`transact(frame, timeoutMs)`,
 * `read(timeoutMs)`, `close()`) with three implementations: WebUSB, WebHID and the demo
 * meter. Each one is driven through the interface alone: CONNECT gets an ACCEPT, and
 * after `close()` a transfer fails as `disconnected`.
 */

import { describe, expect, it } from "vitest";
import { DemoTransport } from "./demoTransport";
import { FakeHidDevice, FakeUsbDevice, toReports } from "./fakeDevices.test";
import { loadWasm } from "./testWasm.test";
import type { WebTransport } from "./transport";
import { openWebHid } from "./webhid";
import { openWebUsb } from "./webusb";

async function implementations(): Promise<[string, WebTransport][]> {
  const w = await loadWasm();
  const usbMeter = new w.DemoMeter("idle");
  const usb = new FakeUsbDevice((f) => {
    const r = usbMeter.transact(f);
    return r.length ? r : null;
  });
  const hidMeter = new w.DemoMeter("idle");
  const hid = new FakeHidDevice((report) => {
    // The meter reads the frame from the front of the padded report: CONNECT is 4 bytes.
    const r = hidMeter.transact(report.slice(0, 4));
    return toReports(r);
  });
  return [
    ["webusb", await openWebUsb(usb as unknown as Parameters<typeof openWebUsb>[0])],
    ["webhid", await openWebHid(hid as unknown as Parameters<typeof openWebHid>[0])],
    ["demo", new DemoTransport(w, "idle", () => 0)],
  ];
}

describe("WebTransport (AC-QFTCWZ.1)", () => {
  it("has three implementations behind one interface", async () => {
    const all = await implementations();
    expect(all.map(([n]) => n)).toEqual(["webusb", "webhid", "demo"]);
    for (const [, t] of all) {
      expect(typeof t.transact).toBe("function");
      expect(typeof t.read).toBe("function");
      expect(typeof t.close).toBe("function");
    }
  });

  it("each one carries CONNECT to an ACCEPT and is closed by close()", async () => {
    const w = await loadWasm();
    for (const [name, t] of await implementations()) {
      const reply = await t.transact(w.connectRequest(1), 500);
      expect(reply[0], name).toBe(0x05);
      expect(reply[1], name).toBe(0x01);
      await t.close();
      await expect(t.transact(w.connectRequest(2), 500), name).rejects.toMatchObject({
        kind: "disconnected",
      });
    }
  });
});
