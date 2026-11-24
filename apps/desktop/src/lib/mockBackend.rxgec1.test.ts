/**
 * FR RXGEC1 (AC .3, .4): the mock backend's label and connect refusal of a meter on the
 * old bootloader, the same texts as the real backend's (`rxgec1_connect_refusal.rs`).
 */

import { describe, expect, it } from "vitest";
import type { DeviceListing } from "./backend";
import { MockBackend } from "./mockBackend";

const REFUSAL =
  "This meter is on an old (2.1-style) bootloader, which the desktop app cannot connect to or update. Unplug the meter and plug it back in (no buttons) to return to the application firmware. An experimental update, tested only in simulation, is available from the CLI: `powerz firmware flash FILE --i-understand --experimental-old-bootloader`.";

async function oldBootloaderMeter(backend: MockBackend): Promise<DeviceListing> {
  const meter = (await backend.listDevices()).find((d) => d.mode === "legacy_bootloader");
  if (meter === undefined) throw new Error("the mock lists no old-bootloader meter");
  return meter;
}

describe("MockBackend old-bootloader texts (RXGEC1)", () => {
  it("labels the meter as an experimental CLI update (AC .3)", async () => {
    const meter = await oldBootloaderMeter(new MockBackend("legacy-bootloader"));
    expect(meter.label).toBe("KM003C — MOCK-LEGACY-0001 (old bootloader, experimental CLI update)");
  });

  it("refuses to connect with the backend's text (AC .4)", async () => {
    const backend = new MockBackend("legacy-bootloader");
    const meter = await oldBootloaderMeter(backend);
    await expect(backend.connect(meter.id, 50, () => {})).rejects.toMatchObject({
      message: REFUSAL,
    });
  });
});
