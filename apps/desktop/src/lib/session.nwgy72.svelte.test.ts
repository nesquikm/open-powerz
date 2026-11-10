/**
 * FR NWGY72: after a meter replug, Connect must not silently use the demo device.
 *
 * - AC-NWGY72.1: when the picked real meter leaves the device list while no session runs,
 *   it stays listed and picked, labelled as not connected; the pick never moves to a demo
 *   device on its own (desktop and web ids).
 * - AC-NWGY72.2: a listing of that meter coming back (same serial, or the same id without a
 *   serial) is picked again and the not-connected entry goes; a meter that appears while a
 *   demo was picked automatically is picked too.
 * - AC-NWGY72.3 (session side): while a not-connected meter is picked, Connect is disabled.
 *
 * The desktop sequence is the HIL13 one: the meter is picked at startup, connected,
 * disconnected, then unplugged, so the list holds only the demos.
 */

import { describe, expect, it } from "vitest";
import type { BackendEvent, DeviceListing } from "./backend";
import { canConnectTo } from "./deviceDetails";
import { deviceOptionLabel } from "./format";
import { Session } from "./session.svelte";

const SERIAL = "SERIAL-NWGY72";

function meter(id: string, serial: string | null): DeviceListing {
  return {
    id,
    label: "POWER-Z KM003C",
    model: "KM003C",
    serial,
    bus: id.startsWith("usb:") ? "1" : null,
    address: id.startsWith("usb:") ? 7 : null,
    demo_scenario: null,
    mode: "application",
  };
}

function demo(name: string): DeviceListing {
  return {
    id: `demo:${name}`,
    label: `Demo device — ${name}`,
    model: "KM003C",
    serial: "123456",
    bus: null,
    address: null,
    demo_scenario: name,
    mode: "application",
  };
}

const DEMOS = [demo("idle"), demo("load-5v-1a")];

function connectedTo(serial: string): BackendEvent {
  return {
    type: "status",
    data: {
      state: "connected",
      model: "KM003C",
      firmware_version: "2.0.6",
      bootloader_version: "",
      mode: "application",
      warnings: [],
      update_required: false,
      firmware_problems: [],
      serial,
      rate_sps: 1000,
      rate_locked: false,
      session_id: 3,
      min_supported_firmware: "1.7.0",
      known_latest_firmware: "2.0.7",
    },
  };
}

const disconnected: BackendEvent = { type: "status", data: { state: "disconnected", error: null } };

/** The picked entry, or undefined when none. */
function picked(s: Session): DeviceListing | undefined {
  return s.devices.find((d) => d.id === s.selectedId);
}

/** HIL13: auto-picked at startup, connected, disconnected; no session runs now. */
function connectedThenDisconnected(id: string): Session {
  const s = new Session();
  s.setDevices([meter(id, SERIAL), ...DEMOS]);
  expect(s.selectedId).toBe(id);
  s.beginConnect();
  s.handleEvent(connectedTo(SERIAL));
  s.handleEvent(disconnected);
  expect(s.state).toBe("disconnected");
  return s;
}

describe("A vanished picked meter stays picked as not connected (AC-NWGY72.1)", () => {
  it("keeps the desktop meter picked after connect, disconnect and unplug (HIL13)", () => {
    const s = connectedThenDisconnected("usb:1:7");
    s.setDevices(DEMOS);
    expect(s.selectedId).toBe("usb:1:7");
    expect(s.selectedId).not.toBe("demo:idle");
    const entry = picked(s);
    expect(entry?.demo_scenario).toBeNull();
    expect(entry?.serial).toBe(SERIAL);
    expect(entry?.absent).toBe(true);
    expect(s.devices.map((d) => d.id)).toEqual(["usb:1:7", "demo:idle", "demo:load-5v-1a"]);
  });

  it("keeps a meter picked by hand when it leaves before any connect", () => {
    const s = new Session();
    s.setDevices([...DEMOS, meter("usb:1:7", SERIAL)]);
    s.select("usb:1:7");
    s.setDevices(DEMOS);
    expect(s.selectedId).toBe("usb:1:7");
    expect(picked(s)?.absent).toBe(true);
  });

  it("keeps the web meter picked when the browser stops listing it", () => {
    const s = connectedThenDisconnected("webusb:1");
    s.setDevices(DEMOS);
    expect(s.selectedId).toBe("webusb:1");
    expect(picked(s)?.absent).toBe(true);
  });

  it("keeps it picked through further list updates without it", () => {
    const s = connectedThenDisconnected("usb:1:7");
    s.setDevices(DEMOS);
    s.setDevices([...DEMOS]);
    s.setDevices(DEMOS);
    expect(s.selectedId).toBe("usb:1:7");
    expect(s.devices.filter((d) => d.absent === true)).toHaveLength(1);
  });

  it("labels the stand-in as not connected, also with serial numbers hidden", () => {
    const s = connectedThenDisconnected("usb:1:7");
    s.setDevices(DEMOS);
    const entry = picked(s);
    expect(entry).toBeDefined();
    if (!entry) return;
    expect(deviceOptionLabel(entry, false, s.devices)).toBe(`KM003C — ${SERIAL} — not connected`);
    expect(deviceOptionLabel(entry, true, s.devices)).toBe("KM003C — not connected");
  });

  it("still falls back to another real meter that is listed", () => {
    const s = new Session();
    s.setDevices([meter("usb:1:7", SERIAL), meter("usb:1:8", "SERIAL-OTHER"), ...DEMOS]);
    s.select("usb:1:8");
    s.setDevices([meter("usb:1:7", SERIAL), ...DEMOS]);
    expect(s.selectedId).toBe("usb:1:7");
    expect(s.devices.some((d) => d.absent === true)).toBe(false);
  });
});

describe("The meter is picked again when it comes back (AC-NWGY72.2)", () => {
  it("picks the desktop meter again under its new id and drops the stand-in", () => {
    const s = connectedThenDisconnected("usb:1:7");
    s.setDevices(DEMOS);
    s.setDevices([meter("usb:1:9", SERIAL), ...DEMOS]);
    expect(s.selectedId).toBe("usb:1:9");
    expect(s.devices.map((d) => d.id)).toEqual(["usb:1:9", "demo:idle", "demo:load-5v-1a"]);
    expect(s.devices.some((d) => d.absent === true)).toBe(false);
  });

  it("picks the web meter again under its new id", () => {
    const s = connectedThenDisconnected("webusb:1");
    s.setDevices(DEMOS);
    s.setDevices([meter("webusb:2", SERIAL), ...DEMOS]);
    expect(s.selectedId).toBe("webusb:2");
    expect(s.devices.map((d) => d.id)).toEqual(["webusb:2", "demo:idle", "demo:load-5v-1a"]);
  });

  it("picks the same id again for a meter without a serial", () => {
    const s = new Session();
    s.setDevices([meter("usb:1:7", null), ...DEMOS]);
    s.select("usb:1:7");
    s.setDevices(DEMOS);
    expect(s.selectedId).toBe("usb:1:7");
    expect(picked(s)?.absent).toBe(true);
    s.setDevices([meter("usb:1:7", null), ...DEMOS]);
    expect(s.selectedId).toBe("usb:1:7");
    expect(picked(s)?.absent).not.toBe(true);
    expect(s.devices.filter((d) => d.id === "usb:1:7")).toHaveLength(1);
  });

  it("can connect again once the meter is back", () => {
    const s = connectedThenDisconnected("usb:1:7");
    s.setDevices(DEMOS);
    s.setDevices([meter("usb:1:9", SERIAL), ...DEMOS]);
    expect(s.canToggleConnect).toBe(true);
  });

  it("picks a meter that appears while a demo was picked automatically", () => {
    const s = new Session();
    s.setDevices(DEMOS);
    expect(s.selectedId).toBe("demo:idle");
    s.setDevices([...DEMOS, meter("usb:1:7", SERIAL)]);
    expect(s.selectedId).toBe("usb:1:7");
  });

  it("picks a web meter that appears while a demo was picked automatically", () => {
    const s = new Session();
    s.setDevices(DEMOS);
    s.setDevices([meter("webusb:1", SERIAL), ...DEMOS]);
    expect(s.selectedId).toBe("webusb:1");
  });

  it("keeps a demo the user picked when a meter appears", () => {
    const s = new Session();
    s.setDevices(DEMOS);
    s.select("demo:load-5v-1a");
    s.setDevices([meter("usb:1:7", SERIAL), ...DEMOS]);
    expect(s.selectedId).toBe("demo:load-5v-1a");
  });
});

describe("Connect is disabled while a not-connected meter is picked (AC-NWGY72.3)", () => {
  it("disables Connect for the stand-in", () => {
    const s = connectedThenDisconnected("usb:1:7");
    s.setDevices(DEMOS);
    expect(s.canToggleConnect).toBe(false);
  });

  it("canConnectTo refuses an absent listing", () => {
    expect(canConnectTo({ ...meter("usb:1:7", SERIAL), absent: true })).toBe(false);
    expect(canConnectTo(meter("usb:1:7", SERIAL))).toBe(true);
  });
});
