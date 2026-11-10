/**
 * FR EHY1JX, AC-EHY1JX.2: a web HID entry reads its model followed by "(HID)" in the
 * device menu, with serial numbers shown or hidden; other entries do not say HID.
 */

import { describe, expect, it } from "vitest";
import type { DeviceListing } from "./backend";
import { deviceOptionLabel } from "./format";

const hidEntry: DeviceListing = {
  id: "webhid:1",
  label: "KM003C (HID)",
  model: "KM003C",
  serial: null,
  bus: null,
  address: null,
  demo_scenario: null,
  mode: "application",
};

const usbEntry: DeviceListing = {
  id: "webusb:1",
  label: "KM003C — 123456",
  model: "KM003C",
  serial: "123456",
  bus: null,
  address: null,
  demo_scenario: null,
  mode: "application",
};

describe("the HID entry's menu label (AC-EHY1JX.2)", () => {
  it("reads the model followed by (HID) with serials shown", () => {
    expect(deviceOptionLabel(hidEntry, false)).toBe("KM003C (HID)");
    expect(deviceOptionLabel(hidEntry, false, [usbEntry, hidEntry])).toBe("KM003C (HID)");
  });

  it("reads the model followed by (HID) with serials hidden", () => {
    expect(deviceOptionLabel(hidEntry, true)).toBe("KM003C (HID)");
  });

  it("keeps (HID) last when hidden serials number two meters of one model", () => {
    const label = deviceOptionLabel(hidEntry, true, [usbEntry, hidEntry]);
    expect(label.startsWith("KM003C")).toBe(true);
    expect(label.endsWith("(HID)")).toBe(true);
  });

  it("says HID for a KM002C HID entry too", () => {
    expect(deviceOptionLabel({ ...hidEntry, model: "KM002C" }, false)).toBe("KM002C (HID)");
  });

  it("leaves WebUSB and desktop entries without (HID)", () => {
    expect(deviceOptionLabel(usbEntry, false)).toBe("KM003C — 123456");
    expect(deviceOptionLabel(usbEntry, true)).toBe("KM003C");
    expect(deviceOptionLabel({ ...usbEntry, id: "usb:1:4" }, false)).not.toContain("HID");
  });
});
