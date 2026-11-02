import { describe, expect, it } from "vitest";
import type { DeviceListing, Sample } from "./backend";
import {
  currentDirectionText,
  DASH,
  badgeLabel,
  deviceOptionLabel,
  displayMeterLabel,
  displaySerial,
  formatCurrent,
  formatFixed,
  readoutView,
  sortDevices,
  temperatureDisplay,
} from "./format";

const sample: Sample = {
  t_ms: 1000,
  vbus_v: 20.0049,
  ibus_a: 2,
  cc1_v: 1.6804,
  cc2_v: 0,
  dp_v: 0.6,
  dm_v: 0.0004,
};

describe("formatFixed (AC-AH2AEK.2)", () => {
  it("renders the requested decimals", () => {
    expect(formatFixed(5, 3)).toBe("5.000");
    expect(formatFixed(36.25, 1)).toBe("36.3");
  });

  it("shows a dash for missing values", () => {
    expect(DASH).toBe("—");
    expect(formatFixed(null, 3)).toBe("—");
    expect(formatFixed(undefined, 3)).toBe("—");
    expect(formatFixed(Number.NaN, 3)).toBe("—");
  });

  it("never renders a negative zero", () => {
    expect(formatFixed(-0.0001, 3)).toBe("0.000");
    // A displayed negative uses the true minus sign (U+2212), like the stats panel.
    expect(formatFixed(-1.169, 3)).toBe("−1.169");
  });
});

describe("formatCurrent (AC-AH2AEK.2)", () => {
  it("keeps the sign and points the arrow in the current direction", () => {
    expect(formatCurrent(1.2346)).toEqual({ text: "1.235", arrow: "→" });
    expect(formatCurrent(-0.5)).toEqual({ text: "−0.500", arrow: "←" });
  });

  it("shows no arrow for zero and a dash when disconnected", () => {
    expect(formatCurrent(0)).toEqual({ text: "0.000", arrow: "" });
    expect(formatCurrent(null)).toEqual({ text: "—", arrow: "" });
  });
});

describe("readoutView (AC-AH2AEK.2)", () => {
  it("formats every live value with its precision", () => {
    const view = readoutView(sample, 36.27, 50);
    expect(view.vbus).toBe("20.005");
    expect(view.ibus).toBe("2.000");
    expect(view.ibusArrow).toBe("→");
    expect(view.power).toBe("40.010");
    expect(view.temp).toBe("36.3");
    expect(view.tempTooltip).toBeNull();
    expect(view.dp).toBe("0.600");
    expect(view.dm).toBe("0.000");
    expect(view.cc1).toBe("1.680");
    expect(view.cc2).toBe("0.000");
  });

  it("shows a dash in every field while disconnected", () => {
    const view = readoutView(null, null, null);
    for (const field of ["vbus", "ibus", "power", "temp", "dp", "dm", "cc1", "cc2"] as const) {
      expect(view[field]).toBe("—");
    }
    expect(view.ibusArrow).toBe("");
  });
});

describe("temperatureDisplay (AC-AH2AEK.9, AC-X2SPDW.6)", () => {
  it("shows the temperature with one decimal when reported", () => {
    expect(temperatureDisplay(25.04, 50)).toEqual({ text: "25.0", tooltip: null });
  });

  it("shows the latest snapshot temperature at 1000 SPS, updated once a second", () => {
    expect(temperatureDisplay(30.64, 1000)).toEqual({
      text: "30.6",
      tooltip: "Updated once a second at 1000 SPS",
    });
  });

  it("shows the temperature in the readout at 1000 SPS", () => {
    const view = readoutView(sample, 30.6, 1000);
    expect(view.temp).toBe("30.6");
    expect(view.tempTooltip).toBe("Updated once a second at 1000 SPS");
  });

  it("keeps no tooltip for a reported temperature at other rates", () => {
    for (const rate of [2, 10, 50, null]) {
      expect(temperatureDisplay(30.6, rate)).toEqual({ text: "30.6", tooltip: null });
    }
  });

  it("explains the 1000 SPS gap when the meter reports no temperature", () => {
    expect(temperatureDisplay(null, 1000)).toEqual({
      text: "—",
      tooltip: "Not reported by the meter at 1000 SPS",
    });
  });

  it("uses the generic tooltip at other rates", () => {
    for (const rate of [2, 10, 50, null]) {
      expect(temperatureDisplay(null, rate)).toEqual({
        text: "—",
        tooltip: "Not reported by the meter",
      });
    }
  });

  it("never shows zero for an unreported temperature in the readout", () => {
    const view = readoutView(sample, null, 1000);
    expect(view.temp).toBe("—");
    expect(view.tempTooltip).toBe("Not reported by the meter at 1000 SPS");
  });
});

describe("device picker labels (AC-AH2AEK.3)", () => {
  const real: DeviceListing = {
    id: "usb:1:4",
    label: "KM003C — 12345678",
    model: "KM003C",
    serial: "12345678",
    bus: "1",
    address: 4,
    demo_scenario: null,
    mode: "application",
  };
  const demo: DeviceListing = {
    id: "demo:pd-negotiation",
    label: "Demo device — pd-negotiation",
    model: "KM003C",
    serial: "SIM0001",
    bus: null,
    address: null,
    demo_scenario: "pd-negotiation",
    mode: "application",
  };

  it("names real meters by model and serial", () => {
    expect(deviceOptionLabel(real, false)).toBe("KM003C — 12345678");
  });

  it("names a real meter without a serial by model", () => {
    expect(deviceOptionLabel({ ...real, serial: null }, false)).toBe("KM003C");
  });

  it("names demo entries by scenario", () => {
    expect(deviceOptionLabel(demo, false)).toBe("Demo device — pd-negotiation");
  });

  it("reads Hidden in place of the serial with the option on (AC-AXHK2Q.2, .3)", () => {
    expect(deviceOptionLabel(real, true)).toBe("KM003C — Hidden");
    expect(deviceOptionLabel({ ...real, mode: "bootloader" }, true)).toBe(
      "KM003C — Hidden (bootloader mode)",
    );
  });

  it("names real meters by their serial again with the option off (AC-AXHK2Q.3)", () => {
    expect(deviceOptionLabel(real, false)).toBe("KM003C — 12345678");
  });

  it("keeps a demo entry's name, which holds no serial, with the option on", () => {
    expect(deviceOptionLabel(demo, true)).toBe("Demo device — pd-negotiation");
  });

  it("lists real meters before demo devices", () => {
    expect(sortDevices([demo, real]).map((d) => d.id)).toEqual(["usb:1:4", "demo:pd-negotiation"]);
  });
});

describe("badgeLabel (AC-AH2AEK.3)", () => {
  it("names every connection state", () => {
    expect(badgeLabel("disconnected")).toBe("Disconnected");
    expect(badgeLabel("connecting")).toBe("Connecting");
    expect(badgeLabel("streaming")).toBe("Streaming");
    expect(badgeLabel("rate_locked")).toBe("Rate locked");
    expect(badgeLabel("error")).toBe("Error");
  });
});

describe("current direction text (orchestrator visual review)", () => {
  it("names the direction the arrow shows, and nothing at zero", () => {
    expect(currentDirectionText(1.2)).toBe("Current flows source → sink");
    expect(currentDirectionText(-0.2)).toBe("Current flows sink → source (reversed)");
    expect(currentDirectionText(0)).toBeNull();
    expect(currentDirectionText(null)).toBeNull();
  });
});

describe("displaySerial (AC-AXHK2Q.2, .3)", () => {
  it("reads Hidden with the option on", () => {
    expect(displaySerial("FAKE-SERIAL-0001", true)).toBe("Hidden");
  });

  it("gives the serial back with the option off", () => {
    expect(displaySerial("FAKE-SERIAL-0001", false)).toBe("FAKE-SERIAL-0001");
  });
});

describe("displayMeterLabel", () => {
  it("keeps the model and hides what follows it (AC-AXHK2Q.2)", () => {
    expect(displayMeterLabel("KM003C — FAKE-SERIAL", true)).toBe("KM003C — Hidden");
  });

  it("hides a whole label it cannot read, not shows it", () => {
    expect(displayMeterLabel("FAKE-SERIAL", true)).toBe("Hidden");
  });

  it("leaves the label alone with the option off", () => {
    expect(displayMeterLabel("KM003C — FAKE-SERIAL", false)).toBe("KM003C — FAKE-SERIAL");
  });
});
