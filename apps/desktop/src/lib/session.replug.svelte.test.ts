/**
 * FR R13Y5V: a replugged meter leaves no dead entry in the device menu.
 *
 * - AC-R13Y5V.1: when the picked meter leaves the list after its session ended and a
 *   meter with the same serial appears, the selection moves to the new entry and the
 *   old entry is dropped.
 * - AC-R13Y5V.2: with no same-serial replacement, a vanished picked meter still stays
 *   listed and picked while its session runs or has just ended with an error.
 * - AC-R13Y5V.4: the desktop wait-for-replug flow is covered unchanged by
 *   `session.svelte.test.ts` ("Session waits for an unplugged meter"); one desktop
 *   sequence is repeated here next to the browser one as a guard.
 *
 * The browser sequence is the one of web HIL run WEB.hil2-20261108-2223 (W6): the
 * WebUSB meter at `webusb:1` streams, the cable is pulled ("The meter is
 * disconnected."), and after the replug the browser lists it again as `webusb:2`.
 */

import { describe, expect, it } from "vitest";
import type { BackendEvent, DeviceListing } from "./backend";
import { Session } from "./session.svelte";

const SERIAL = "SERIAL-R13Y5V";
const OTHER = "SERIAL-OTHER";

function meter(id: string, serial: string): DeviceListing {
  return {
    id,
    label: "POWER-Z KM003C",
    model: "KM003C",
    serial,
    bus: null,
    address: null,
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

/** The browser ends the session with the unplug error and never sends `waiting`. */
const unpluggedWeb: BackendEvent = {
  type: "status",
  data: {
    state: "disconnected",
    error: {
      kind: "disconnected",
      message: "The meter is disconnected.",
      hint: "Plug it in and connect again.",
    },
  },
};

/** Streaming from the meter at `webusb:1`. */
function streamingWeb(): Session {
  const s = new Session();
  s.setDevices([meter("webusb:1", SERIAL), ...DEMOS]);
  s.select("webusb:1");
  s.beginConnect();
  s.handleEvent(connectedTo(SERIAL));
  return s;
}

describe("Session follows a replugged meter by serial (AC-R13Y5V.1)", () => {
  it("moves the selection to the replugged meter and drops the dead entry (HIL W6)", () => {
    const s = streamingWeb();
    // The browser's disconnect event lists the meter as gone before the session ends.
    s.setDevices(DEMOS);
    s.handleEvent(unpluggedWeb);
    expect(s.state).toBe("error");
    // Replug: the browser lists the same meter under a new id.
    s.setDevices([meter("webusb:2", SERIAL), ...DEMOS]);
    expect(s.selectedId).toBe("webusb:2");
    expect(s.devices.map((d) => d.id)).toEqual(["webusb:2", "demo:idle", "demo:load-5v-1a"]);
  });

  it("follows the replugged meter when the list lands only after the session ended", () => {
    const s = streamingWeb();
    s.handleEvent(unpluggedWeb);
    s.setDevices([meter("webusb:2", SERIAL), ...DEMOS]);
    expect(s.selectedId).toBe("webusb:2");
    expect(s.devices.some((d) => d.id === "webusb:1")).toBe(false);
  });

  it("follows the serial, not the first meter, when another meter is listed too", () => {
    const s = streamingWeb();
    s.setDevices(DEMOS);
    s.handleEvent(unpluggedWeb);
    s.setDevices([meter("webusb:3", OTHER), meter("webusb:2", SERIAL), ...DEMOS]);
    expect(s.selectedId).toBe("webusb:2");
    expect(s.devices.map((d) => d.id)).not.toContain("webusb:1");
    expect(s.devices.filter((d) => d.serial === SERIAL)).toHaveLength(1);
  });

  it("follows a meter that came back over WebHID", () => {
    const s = streamingWeb();
    s.setDevices(DEMOS);
    s.handleEvent(unpluggedWeb);
    s.setDevices([meter("webhid:1", SERIAL), ...DEMOS]);
    expect(s.selectedId).toBe("webhid:1");
    expect(s.devices.map((d) => d.id)).toEqual(["webhid:1", "demo:idle", "demo:load-5v-1a"]);
  });

  it("follows the replugged meter after a session that ended without an error", () => {
    const s = streamingWeb();
    s.setDevices([meter("webusb:3", OTHER), ...DEMOS]);
    s.handleEvent({ type: "status", data: { state: "disconnected", error: null } });
    s.setDevices([meter("webusb:3", OTHER), meter("webusb:2", SERIAL), ...DEMOS]);
    expect(s.selectedId).toBe("webusb:2");
    expect(s.devices.map((d) => d.id)).not.toContain("webusb:1");
  });
});

describe("Session keeps a vanished meter with no same-serial replacement (AC-R13Y5V.2)", () => {
  it("keeps it listed and picked while its session runs", () => {
    const s = streamingWeb();
    s.setDevices([meter("webusb:3", OTHER), ...DEMOS]);
    expect(s.selectedId).toBe("webusb:1");
    expect(s.devices.find((d) => d.id === "webusb:1")?.serial).toBe(SERIAL);
    expect(s.devices.map((d) => d.id)).toContain("webusb:3");
  });

  it("keeps it listed and picked after the session ended with an error", () => {
    const s = streamingWeb();
    s.setDevices(DEMOS);
    s.handleEvent(unpluggedWeb);
    s.setDevices([meter("webusb:3", OTHER), ...DEMOS]);
    expect(s.selectedId).toBe("webusb:1");
    expect(s.devices.find((d) => d.id === "webusb:1")?.serial).toBe(SERIAL);
    // Never falls back to a demo or the other meter.
    s.setDevices(DEMOS);
    expect(s.selectedId).toBe("webusb:1");
  });
});

describe("Desktop wait-for-replug is unchanged (AC-R13Y5V.4)", () => {
  it("keeps the meter picked while waiting and follows it when the OS lists it again", () => {
    const s = new Session();
    s.setDevices([meter("usb:1:7", SERIAL), ...DEMOS]);
    s.select("usb:1:7");
    s.beginConnect();
    s.handleEvent(connectedTo(SERIAL));
    s.handleEvent({
      type: "status",
      data: {
        state: "disconnected",
        error: { kind: "disconnected", message: "The meter was unplugged.", hint: null },
      },
    });
    s.handleEvent({ type: "status", data: { state: "waiting", serial: SERIAL } });
    s.setDevices([meter("usb:1:8", OTHER), ...DEMOS]);
    expect(s.state).toBe("waiting");
    expect(s.selectedId).toBe("usb:1:7");
    s.setDevices([meter("usb:1:8", OTHER), meter("usb:1:9", SERIAL), ...DEMOS]);
    expect(s.selectedId).toBe("usb:1:9");
    expect(s.state).toBe("waiting");
    expect(s.waitingSerial).toBe(SERIAL);
  });
});
