/** FR 1HHT03, AC .2: the grid of meter tiles. */

import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import type { DeviceListing } from "./backend";
import { LAYOUT_BREAKPOINT_PX } from "./layout";
import { controlsLabel, serialSuffix, tileTitle } from "./meterGrid";

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

const SHOWN = { hidden: false, position: 1 };

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
    expect(tileTitle(listing({}), null, SHOWN)).toEqual({ name: "KM003C", suffix: "…0007" });
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
    expect(tileTitle(demo, null, SHOWN)).toEqual({
      name: "Demo device — pd-9v-2a",
      suffix: "…3456",
    });
  });

  it("prefers the serial the connected meter reported", () => {
    // A second simulated meter reports a serial of its own (the serial override).
    const demo = listing({ serial: "123456", demo_scenario: "idle", label: "Demo device — idle" });
    expect(tileTitle(demo, "SIM-B-0002", SHOWN).suffix).toBe("…0002");
  });
});

describe("tileTitle with serial numbers hidden (AC-AXHK2Q.4)", () => {
  it("titles a meter by model and position, with no suffix", () => {
    expect(tileTitle(listing({}), "SERIAL-0007", { hidden: true, position: 2 })).toEqual({
      name: "KM003C 2",
      suffix: "",
    });
  });

  it("keeps two meters with no serial apart", () => {
    const noSerial = listing({ serial: null });
    const first = tileTitle(noSerial, null, { hidden: true, position: 1 });
    const second = tileTitle(noSerial, null, { hidden: true, position: 2 });
    expect(first.name).toBe("KM003C 1");
    expect(second.name).toBe("KM003C 2");
  });

  it("titles a demo meter the same way", () => {
    const demo = listing({ serial: "123456", demo_scenario: "pd-9v-2a" });
    expect(tileTitle(demo, "123456", { hidden: true, position: 3 })).toEqual({
      name: "KM003C 3",
      suffix: "",
    });
  });

  it("carries no serial into the control bar's name", () => {
    const title = tileTitle(listing({}), "SERIAL-0007", { hidden: true, position: 2 });
    expect(controlsLabel(title)).toBe("Controls for KM003C 2");
  });

  it("is the ordinary title with the option off (AC-AXHK2Q.3)", () => {
    expect(tileTitle(listing({}), "SERIAL-0007", { hidden: false, position: 2 })).toEqual({
      name: "KM003C",
      suffix: "…0007",
    });
  });
});

describe("the tile grid's columns (AC-1HHT03.2)", () => {
  it("go side by side at the app's layout breakpoint, in CSS", () => {
    const css = readFileSync(new URL("./components/MeterGrid.svelte", import.meta.url), "utf8");
    expect(css).toContain(`@media (min-width: ${LAYOUT_BREAKPOINT_PX}px)`);
  });
});
