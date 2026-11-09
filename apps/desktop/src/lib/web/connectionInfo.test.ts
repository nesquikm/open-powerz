/**
 * FR 60QB2S: the (i) beside the web device menu explains the connections. These tests
 * cover the pure helper the popover reads its text from (`./connectionInfo`).
 *
 * - AC-60QB2S.3: short sentences on WebUSB (full feature set: live readings, PD capture,
 *   rates, records, logo), HID ("Connect over HID…": basic readings only, works while
 *   another program holds the meter) and Web Serial (settings changes and the protocol
 *   trigger only; Chrome asks once for the meter's serial port).
 * - AC-60QB2S.4: Chrome or Edge can connect; Safari and Firefox show the demo only.
 * - AC-60QB2S.5: the current session's connection: WebUSB, HID, the demo meter, or none yet.
 */

import { describe, expect, it } from "vitest";
import type { ConnectedInfo, DeviceListing } from "../backend";
import type { ConnectionState } from "../format";

interface SessionLike {
  devices: readonly DeviceListing[];
  selectedId: string | null;
  state: ConnectionState;
  details?: Partial<ConnectedInfo> | null;
}

interface ConnectionInfoModule {
  CONNECTION_INFO_LINES: readonly string[];
  currentConnectionLine(session: SessionLike): string;
}

/** The module under test, named through a variable so the suite type-checks before it
 * exists. */
const MODULE = "./connectionInfo";
const load = async (): Promise<ConnectionInfoModule> =>
  (await import(/* @vite-ignore */ MODULE)) as ConnectionInfoModule;

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

const REAL: DeviceListing = {
  id: "webusb:1",
  label: "KM003C — WEB-0001",
  model: "KM003C",
  serial: "WEB-0001",
  bus: null,
  address: null,
  demo_scenario: null,
  mode: "application",
};

const HID_DEVICE: DeviceListing = { ...REAL, id: "webhid:1" };

/** The line among the popover lines that matches `pattern`. */
async function lineMatching(pattern: RegExp): Promise<string> {
  const { CONNECTION_INFO_LINES } = await load();
  const line = CONNECTION_INFO_LINES.find((l) => pattern.test(l));
  expect(line, `a line matching ${pattern}`).toBeDefined();
  return line ?? "";
}

describe("the connection lines (AC-60QB2S.3)", () => {
  it("is a short list of short sentences", async () => {
    const { CONNECTION_INFO_LINES } = await load();
    expect(CONNECTION_INFO_LINES.length).toBeGreaterThanOrEqual(4);
    expect(CONNECTION_INFO_LINES.length).toBeLessThanOrEqual(8);
    for (const line of CONNECTION_INFO_LINES) {
      expect(line.trim().length, line).toBeGreaterThan(0);
      expect(line.length, `short: ${line}`).toBeLessThanOrEqual(200);
      expect(line, `a sentence: ${line}`).toMatch(/[.!]$/);
    }
  });

  it("WebUSB gives the full feature set: live readings, PD capture, rates, records, logo", async () => {
    const line = await lineMatching(/\bWebUSB\b/);
    expect(line).toMatch(/full feature set|every feature|all features/i);
    expect(line).toMatch(/live readings/i);
    expect(line).toMatch(/PD capture/i);
    expect(line).toMatch(/rates?\b/i);
    expect(line).toMatch(/records/i);
    expect(line).toMatch(/logo/i);
  });

  it("HID (Connect over HID…) gives basic readings only and works while another program holds the meter", async () => {
    const line = await lineMatching(/\bHID\b/);
    expect(line).toContain("Connect over HID…");
    expect(line).toMatch(/basic readings only/i);
    expect(line).toMatch(/another program/i);
    expect(line).toMatch(/holds the meter/i);
  });

  it("Web Serial is only for settings changes and the protocol trigger; Chrome asks once for the serial port", async () => {
    const line = await lineMatching(/\bWeb Serial\b/);
    expect(line).toMatch(/\bonly\b/i);
    expect(line).toMatch(/settings changes/i);
    expect(line).toMatch(/protocol trigger/i);
    expect(line).toMatch(/Chrome asks once/i);
    expect(line).toMatch(/serial port/i);
  });
});

describe("which browsers connect (AC-60QB2S.4)", () => {
  it("Chrome or Edge can connect", async () => {
    const line = await lineMatching(/\bChrome or Edge\b/);
    expect(line).toMatch(/connect/i);
  });

  it("Safari and Firefox show the demo only", async () => {
    const line = await lineMatching(/\bSafari\b/);
    expect(line).toMatch(/\bFirefox\b/);
    expect(line).toMatch(/demo only/i);
  });
});

describe("the current session's connection (AC-60QB2S.5)", () => {
  it("names WebUSB for a connected real meter", async () => {
    const { currentConnectionLine } = await load();
    const line = currentConnectionLine({
      devices: [DEMO, REAL],
      selectedId: REAL.id,
      state: "streaming",
      details: { basic_readings_only: false },
    });
    expect(line).toMatch(/\bWebUSB\b/);
    expect(line).not.toMatch(/\bHID\b|demo|none yet/i);
  });

  it("names HID for a session that gives basic readings only", async () => {
    const { currentConnectionLine } = await load();
    const line = currentConnectionLine({
      devices: [DEMO, HID_DEVICE],
      selectedId: HID_DEVICE.id,
      state: "streaming",
      details: { basic_readings_only: true },
    });
    expect(line).toMatch(/\bHID\b/);
    expect(line).not.toMatch(/\bWebUSB\b|demo|none yet/i);
  });

  it("names the demo meter when the demo is selected", async () => {
    const { currentConnectionLine } = await load();
    for (const state of ["streaming", "rate_locked"] as const) {
      const line = currentConnectionLine({
        devices: [DEMO],
        selectedId: DEMO.id,
        state,
        details: null,
      });
      expect(line, state).toMatch(/the demo meter/i);
      expect(line, state).not.toMatch(/\bWebUSB\b|\bHID\b|none yet/i);
    }
  });

  it("says none yet with no device or no connection", async () => {
    const { currentConnectionLine } = await load();
    const empty = currentConnectionLine({
      devices: [],
      selectedId: null,
      state: "disconnected",
      details: null,
    });
    expect(empty).toMatch(/none yet/i);
    const idle = currentConnectionLine({
      devices: [DEMO, REAL],
      selectedId: REAL.id,
      state: "disconnected",
      details: null,
    });
    expect(idle).toMatch(/none yet/i);
    expect(idle).not.toMatch(/\bWebUSB\b|\bHID\b|demo/i);
  });

  it("the four cases give four different lines", async () => {
    const { currentConnectionLine } = await load();
    const lines = new Set([
      currentConnectionLine({
        devices: [REAL],
        selectedId: REAL.id,
        state: "streaming",
        details: {},
      }),
      currentConnectionLine({
        devices: [HID_DEVICE],
        selectedId: HID_DEVICE.id,
        state: "streaming",
        details: { basic_readings_only: true },
      }),
      currentConnectionLine({ devices: [DEMO], selectedId: DEMO.id, state: "streaming" }),
      currentConnectionLine({ devices: [], selectedId: null, state: "disconnected" }),
    ]);
    expect(lines.size).toBe(4);
  });
});
