/**
 * Device details and firmware warnings (FR V9SK52, AC .4 and .5): what the desktop app
 * shows for the connected meter, and which picker entries can be connected.
 */

import { describe, expect, it } from "vitest";
import type { ConnectedInfo, DeviceListing } from "./backend";
import {
  KNOWN_LATEST_FIRMWARE,
  MIN_SUPPORTED_FIRMWARE,
  canConnectTo,
  deviceDetailsRows,
  firmwareWarningText,
  modeLabel,
} from "./deviceDetails";

function info(over: Partial<ConnectedInfo> = {}): ConnectedInfo {
  return {
    model: "KM003C",
    firmware_version: "2.0.6",
    bootloader_version: "2.5",
    mode: "application",
    warnings: ["outdated"],
    update_required: false,
    firmware_problems: [],
    serial: "123456",
    rate_sps: 50,
    rate_locked: false,
    session_id: 1,
    min_supported_firmware: "1.7.0",
    known_latest_firmware: "2.0.7",
    ...over,
  };
}

function listing(mode: DeviceListing["mode"], demo: string | null = null): DeviceListing {
  return {
    id: demo ? `demo:${demo}` : "usb:1:9",
    label: mode === "bootloader" ? "KM003C — bootloader mode" : "KM003C — SERIAL-0009",
    model: "KM003C",
    serial: "SERIAL-0009",
    bus: demo ? null : "1",
    address: demo ? null : 9,
    demo_scenario: demo,
    mode,
  };
}

describe("firmware constants", () => {
  it("match the protocol library's floor and newest known version", () => {
    expect(MIN_SUPPORTED_FIRMWARE).toBe("1.7.0");
    expect(KNOWN_LATEST_FIRMWARE).toBe("2.0.7");
  });
});

describe("modeLabel", () => {
  it("names each mode in plain words", () => {
    expect(modeLabel("application")).toBe("Application");
    expect(modeLabel("bootloader")).toBe("Bootloader");
    expect(modeLabel("unknown")).toBe("Unknown");
  });
});

describe("deviceDetailsRows", () => {
  it("lists the mode and both versions of the connected meter", () => {
    const rows = deviceDetailsRows(info());
    const byLabel = Object.fromEntries(rows.map((r) => [r.label, r.value]));
    expect(byLabel["Mode"]).toBe("Application");
    expect(byLabel["Firmware"]).toBe("2.0.6");
    expect(byLabel["Bootloader"]).toBe("2.5");
    expect(byLabel["Model"]).toBe("KM003C");
  });

  it("shows a dash for a version the meter did not report", () => {
    const rows = deviceDetailsRows(info({ bootloader_version: "" }));
    expect(rows.find((r) => r.label === "Bootloader")?.value).toBe("—");
  });
});

describe("deviceDetailsRows firmware status (AC-V9SK52.4)", () => {
  const value = (i: ConnectedInfo, label: string): string | undefined =>
    deviceDetailsRows(i).find((r) => r.label === label)?.value;
  const problemRows = (i: ConnectedInfo): string[] =>
    deviceDetailsRows(i)
      .filter((r) => r.label === "Firmware status")
      .map((r) => r.value);

  it('shows "Update check" as unavailable when HEAD was not read', () => {
    expect(value(info({ update_required: null }), "Update check")).toBe("unavailable");
  });

  it("does not call a read update check unavailable", () => {
    for (const update_required of [true, false]) {
      const v = value(info({ update_required }), "Update check");
      expect(v).toBeDefined();
      expect(v).not.toBe("unavailable");
    }
    expect(value(info({ update_required: true }), "Update check")).not.toBe(
      value(info({ update_required: false }), "Update check"),
    );
  });

  it("lists each firmware-status problem, in order", () => {
    const problems = [
      "application info block (0x4420) not read: timeout",
      "HEAD not read: the meter sent REJECT (reply: 06 07 00 00)",
    ];
    const rows = problemRows(info({ update_required: null, firmware_problems: problems }));
    expect(rows).toEqual(problems);
  });

  it("lists no problem rows when the status was read in full", () => {
    expect(problemRows(info())).toEqual([]);
  });
});

describe("firmwareWarningText", () => {
  it("outdated names the meter's version and the newest known one", () => {
    const text = firmwareWarningText("outdated", info());
    expect(text).toContain("2.0.6");
    expect(text).toContain("2.0.7");
  });

  it("too old names the 1.7.0 floor", () => {
    const text = firmwareWarningText("too_old", info({ firmware_version: "1.6.0" }));
    expect(text).toContain("1.6.0");
    expect(text).toContain("1.7.0");
  });
});

describe("canConnectTo", () => {
  it("refuses a meter in bootloader mode", () => {
    expect(canConnectTo(listing("bootloader"))).toBe(false);
  });

  it("allows application-mode and unknown-mode meters and the demo device", () => {
    expect(canConnectTo(listing("application"))).toBe(true);
    expect(canConnectTo(listing("unknown"))).toBe(true);
    expect(canConnectTo(listing("application", "load-5v-1a"))).toBe(true);
  });

  it("allows nothing when no device is picked", () => {
    expect(canConnectTo(null)).toBe(false);
  });
});
