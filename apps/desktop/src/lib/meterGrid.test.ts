/** FR 1HHT03, AC .2: the grid of meter tiles. */

import { describe, expect, it } from "vitest";
import type { DeviceListing } from "./backend";
import { LAYOUT_BREAKPOINT_PX } from "./layout";
import { gridColumns, serialSuffix, tileTitle } from "./meterGrid";

function listing(over: Partial<DeviceListing>): DeviceListing {
  return {
    id: "usb:1:7",
    label: "KM003C — SERIAL-0007",
    model: "KM003C",
    serial: "SERIAL-0007",
    bus: "1",
    address: 7,
    demo_scenario: null,
    mode: "application",
    ...over,
  };
}

describe("gridColumns (AC-1HHT03.2)", () => {
  it("puts two meters side by side on a wide window", () => {
    expect(gridColumns(LAYOUT_BREAKPOINT_PX, 2)).toBe(2);
    expect(gridColumns(1920, 2)).toBe(2);
  });

  it("never uses more than two columns, however many meters", () => {
    expect(gridColumns(2560, 3)).toBe(2);
    expect(gridColumns(2560, 4)).toBe(2);
  });

  it("stacks the tiles on a narrow window", () => {
    expect(gridColumns(LAYOUT_BREAKPOINT_PX - 1, 2)).toBe(1);
    expect(gridColumns(800, 3)).toBe(1);
  });

  it("uses one column for a single meter", () => {
    expect(gridColumns(1920, 1)).toBe(1);
    expect(gridColumns(1920, 0)).toBe(1);
  });
});

describe("serialSuffix (AC-1HHT03.2)", () => {
  it("shows the last four characters of the serial", () => {
    expect(serialSuffix("SERIAL-0007")).toBe("…0007");
    expect(serialSuffix("123456")).toBe("…3456");
  });

  it("shows a short serial whole", () => {
    expect(serialSuffix("1234")).toBe("1234");
    expect(serialSuffix("12")).toBe("12");
  });

  it("is empty without a serial", () => {
    expect(serialSuffix(null)).toBe("");
    expect(serialSuffix("")).toBe("");
  });
});

describe("tileTitle (AC-1HHT03.2)", () => {
  it("names a real meter by model and serial suffix", () => {
    expect(tileTitle(listing({}), null)).toEqual({ name: "KM003C", suffix: "…0007" });
  });

  it("names a demo meter by its scenario", () => {
    const demo = listing({
      id: "demo:pd-9v-2a",
      label: "Demo device — pd-9v-2a",
      serial: "123456",
      demo_scenario: "pd-9v-2a",
      bus: null,
      address: null,
    });
    expect(tileTitle(demo, null)).toEqual({ name: "Demo device — pd-9v-2a", suffix: "…3456" });
  });

  it("prefers the serial the connected meter reported", () => {
    // A second simulated meter reports a serial of its own (the serial override).
    const demo = listing({ serial: "123456", demo_scenario: "idle", label: "Demo device — idle" });
    expect(tileTitle(demo, "SIM-B-0002").suffix).toBe("…0002");
  });
});
