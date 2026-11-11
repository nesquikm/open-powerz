/**
 * FR NWGY72, AC-NWGY72.5: `writeTargetName` names the target of a meter write: the demo
 * device by name with a note that no meter is written, or the meter by its picker name,
 * with "serial hidden" when serial numbers are hidden.
 */

import { describe, expect, it } from "vitest";
import type { DeviceListing } from "./backend";
import { writeTargetName } from "./format";

const METER: DeviceListing = {
  id: "usb:1:7",
  label: "POWER-Z KM003C",
  model: "KM003C",
  serial: "1234",
  bus: "1",
  address: 7,
  demo_scenario: null,
  mode: "application",
};

const DEMO: DeviceListing = {
  id: "demo:idle",
  label: "Demo device — idle",
  model: "KM003C",
  serial: "123456",
  bus: null,
  address: null,
  demo_scenario: "idle",
  mode: "application",
};

describe("writeTargetName (AC-NWGY72.5)", () => {
  it("names the demo device and says no meter is written", () => {
    expect(writeTargetName(DEMO, false)).toBe("Demo device — idle (no meter is written)");
  });

  it("names the demo device the same way with serial numbers hidden", () => {
    expect(writeTargetName(DEMO, true, [METER, DEMO])).toBe(
      "Demo device — idle (no meter is written)",
    );
  });

  it("names a meter by its picker name", () => {
    expect(writeTargetName(METER, false, [METER, DEMO])).toBe("KM003C — 1234");
  });

  it("says serial hidden when serial numbers are hidden", () => {
    const name = writeTargetName(METER, true, [METER, DEMO]);
    expect(name).toBe("KM003C — serial hidden");
    expect(name).not.toContain("1234");
  });

  it("never leaks the serial with two meters of one model hidden", () => {
    const other = { ...METER, id: "usb:1:8", serial: "5678" };
    const name = writeTargetName(other, true, [METER, other]);
    expect(name).toContain("serial hidden");
    expect(name).toContain("KM003C");
    expect(name).not.toContain("5678");
  });
});
