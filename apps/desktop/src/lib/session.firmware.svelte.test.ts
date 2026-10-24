/**
 * The session keeps the connected meter's details for the device details view, and
 * a picked bootloader-mode meter cannot be connected (FR V9SK52, AC .5).
 */

import { describe, expect, it } from "vitest";
import type { BackendEvent, DeviceListing } from "./backend";
import { Session } from "./session.svelte";

function connected(): BackendEvent {
  return {
    type: "status",
    data: {
      state: "connected",
      model: "KM003C",
      firmware_version: "2.0.6",
      bootloader_version: "2.5",
      mode: "application",
      warnings: ["outdated"],
      serial: "123456",
      rate_sps: 50,
      rate_locked: false,
      session_id: 1,
    },
  };
}

const BOOT_METER: DeviceListing = {
  id: "usb:1:9",
  label: "KM003C — bootloader mode",
  model: "KM003C",
  serial: null,
  bus: "1",
  address: 9,
  demo_scenario: null,
  mode: "bootloader",
};

const DEMO: DeviceListing = {
  id: "demo:load-5v-1a",
  label: "Demo device — load-5v-1a",
  model: "KM003C",
  serial: "123456",
  bus: null,
  address: null,
  demo_scenario: "load-5v-1a",
  mode: "application",
};

describe("Session device details", () => {
  it("has no details before a connect", () => {
    expect(new Session().details).toBeNull();
  });

  it("keeps the connected meter's mode, versions and warnings", () => {
    const s = new Session();
    s.handleEvent(connected());
    expect(s.details).toMatchObject({
      mode: "application",
      firmware_version: "2.0.6",
      bootloader_version: "2.5",
      warnings: ["outdated"],
    });
  });

  it("drops the details on disconnect", () => {
    const s = new Session();
    s.handleEvent(connected());
    s.handleEvent({ type: "status", data: { state: "disconnected", error: null } });
    expect(s.details).toBeNull();
  });
});

describe("Session with a bootloader-mode meter picked", () => {
  it("lists it and cannot connect to it, without an error", () => {
    const s = new Session();
    s.setDevices([BOOT_METER, DEMO]);
    s.selectedId = BOOT_METER.id;
    expect(s.devices.map((d) => d.id)).toContain(BOOT_METER.id);
    expect(s.canToggleConnect).toBe(false);
    expect(s.error).toBeNull();
  });

  it("can connect again once the demo device is picked", () => {
    const s = new Session();
    s.setDevices([BOOT_METER, DEMO]);
    s.selectedId = DEMO.id;
    expect(s.canToggleConnect).toBe(true);
  });
});
