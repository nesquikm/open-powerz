/**
 * FR R13Y5W: the statistics panel shows another device's numbers.
 *
 * - AC-R13Y5W.2: when a session starts connecting on a device other than the one whose
 *   statistics are shown, the statistics are cleared at once (desktop and web edition:
 *   both feed the panel from `Session.stats`).
 * - AC-R13Y5W.3: reconnecting to the same device keeps today's behaviour: the last
 *   totals stay readable until the new session's first batch replaces them.
 *
 * Evidence: web HIL run WEB.hil2-20261108-2223 (W2): the demo session's min, max and
 * average stayed next to a real meter's (failed) connect.
 */

import { describe, expect, it } from "vitest";
import type { BackendEvent, DeviceListing, Sample, StatsSnapshot } from "./backend";
import { Session } from "./session.svelte";

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

const CONNECTED: BackendEvent = {
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
    serial: "123456",
    rate_sps: 50,
    rate_locked: false,
    session_id: 1,
    min_supported_firmware: "1.7.0",
    known_latest_firmware: "2.0.7",
  },
};

const DISCONNECTED: BackendEvent = {
  type: "status",
  data: { state: "disconnected", error: null },
};

function sample(t_ms: number): Sample {
  return { t_ms, vbus_v: 5, ibus_a: 1, cc1_v: 1.68, cc2_v: 0, dp_v: 0.6, dm_v: 0 };
}

function snap(samples: number): StatsSnapshot {
  return {
    elapsed_ms: samples * 20,
    samples,
    charge_ah: 0.001,
    energy_wh: 0.005,
    vbus: { min: 4.9, max: 5.1, avg: 5 },
    ibus: { min: 0.9, max: 1.1, avg: 1 },
    power: { min: 4.4, max: 5.6, avg: 5 },
  } as StatsSnapshot;
}

function batch(stats: StatsSnapshot): BackendEvent {
  return { type: "batch", data: { samples: [sample(stats.samples)], temp_c: null, stats } };
}

/** A session that streamed on `id` and gathered statistics (`samples` of them). */
function streamedOn(s: Session, id: string, samples = 500): void {
  s.select(id);
  s.beginConnect();
  s.handleEvent(CONNECTED);
  s.handleEvent(batch(snap(samples)));
  expect(s.stats?.samples).toBe(samples);
}

describe("Session stats on another device (AC-R13Y5W.2)", () => {
  it("web edition: connecting a real meter clears the demo's statistics at once", () => {
    const s = new Session();
    s.setDevices([demo("load-5v-1a"), meter("webusb:1", "SERIAL-R13Y5W")]);
    streamedOn(s, "demo:load-5v-1a");
    s.handleEvent(DISCONNECTED);
    // Today's behaviour, kept: the final totals stay readable after a disconnect.
    expect(s.stats?.samples).toBe(500);
    s.select("webusb:1");
    s.beginConnect();
    expect(s.state).toBe("connecting");
    expect(s.stats).toBeNull();
  });

  it("web edition: the panel stays empty when that connect fails", () => {
    const s = new Session();
    s.setDevices([demo("load-5v-1a"), meter("webusb:1", "SERIAL-R13Y5W")]);
    streamedOn(s, "demo:load-5v-1a");
    s.handleEvent(DISCONNECTED);
    s.select("webusb:1");
    s.beginConnect();
    s.failConnect({ kind: "io", message: "The meter did not answer.", hint: null } as never);
    expect(s.stats).toBeNull();
  });

  it("desktop: connecting another USB meter clears the first meter's statistics", () => {
    const s = new Session();
    s.setDevices([meter("usb:1:2", "SERIAL-A"), meter("usb:1:4", "SERIAL-B")]);
    streamedOn(s, "usb:1:2");
    s.handleEvent(DISCONNECTED);
    s.select("usb:1:4");
    s.beginConnect();
    expect(s.stats).toBeNull();
  });

  it("desktop: clears the statistics of an adopted session when another device connects", () => {
    const s = new Session();
    s.setDevices([meter("usb:1:2", "SERIAL-A"), demo("idle")]);
    s.adopt({ device_id: "usb:1:2", rate_sps: 50, rate_locked: false } as never);
    s.handleEvent(batch(snap(900)));
    expect(s.stats?.samples).toBe(900);
    s.handleEvent(DISCONNECTED);
    s.select("demo:idle");
    s.beginConnect();
    expect(s.stats).toBeNull();
  });

  it("the new device's first batch fills the panel again", () => {
    const s = new Session();
    s.setDevices([demo("idle"), meter("webusb:1", "SERIAL-R13Y5W")]);
    streamedOn(s, "demo:idle");
    s.handleEvent(DISCONNECTED);
    s.select("webusb:1");
    s.beginConnect();
    expect(s.stats).toBeNull();
    s.handleEvent(CONNECTED);
    s.handleEvent(batch(snap(3)));
    expect(s.stats?.samples).toBe(3);
  });

  it("a connect on a third device clears the second device's statistics", () => {
    const s = new Session();
    s.setDevices([demo("idle"), demo("load-5v-1a"), meter("webusb:1", "S")]);
    streamedOn(s, "demo:idle");
    s.handleEvent(DISCONNECTED);
    streamedOn(s, "demo:load-5v-1a", 42);
    s.handleEvent(DISCONNECTED);
    expect(s.stats?.samples).toBe(42);
    s.select("webusb:1");
    s.beginConnect();
    expect(s.stats).toBeNull();
  });
});

describe("Session stats on the same device (AC-R13Y5W.3)", () => {
  it("reconnecting the same device keeps the last totals until a new batch comes", () => {
    const s = new Session();
    s.setDevices([demo("idle"), meter("webusb:1", "S")]);
    streamedOn(s, "webusb:1", 700);
    s.handleEvent(DISCONNECTED);
    s.beginConnect();
    expect(s.stats?.samples).toBe(700);
    s.handleEvent(CONNECTED);
    s.handleEvent(batch(snap(1)));
    expect(s.stats?.samples).toBe(1);
  });

  it("picking another device and back before connecting keeps the totals", () => {
    const s = new Session();
    s.setDevices([meter("usb:1:2", "A"), meter("usb:1:4", "B")]);
    streamedOn(s, "usb:1:2", 700);
    s.handleEvent(DISCONNECTED);
    s.select("usb:1:4");
    s.select("usb:1:2");
    s.beginConnect();
    expect(s.stats?.samples).toBe(700);
  });

  it("a reset still blanks the statistics until the next batch", () => {
    const s = new Session();
    s.setDevices([meter("usb:1:2", "A")]);
    streamedOn(s, "usb:1:2", 700);
    s.statsReset();
    expect(s.stats).toBeNull();
    s.handleEvent(batch(snap(2)));
    expect(s.stats?.samples).toBe(2);
  });
});
