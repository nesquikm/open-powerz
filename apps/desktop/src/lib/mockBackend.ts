/**
 * A scripted stand-in for the Tauri backend, so the web build runs in a
 * plain browser (`VITE_BACKEND=mock`): the Playwright UI suite and the
 * visual checks drive it. Its scenarios follow the simulator's shapes in
 * `crates/powerz-sim`; like firmware 2.0.6, it reports no temperature
 * while the queue runs at 1000 SPS.
 */

import type { AppInfo, BackendEvent, DeviceListing, Sample, UiError } from "./backend";

const SCENARIOS = ["idle", "load-5v-1a", "pd-9v-2a", "noisy-negative"] as const;
type Scenario = (typeof SCENARIOS)[number];

/** Batches go out every 25 ms: 40 Hz, inside the backend's 20–60 Hz band. */
const BATCH_MS = 25;

function usage(message: string): UiError {
  return { kind: "usage", message, hint: null };
}

function scenarioOf(deviceId: string): Scenario | undefined {
  const name = deviceId.replace(/^demo:/, "");
  return SCENARIOS.find((s) => s === name);
}

/** The scenario's reading at device time `tMs`. */
function sampleAt(scenario: Scenario, tMs: number): Sample {
  const t = tMs / 1000;
  const lines = { cc1_v: 1.68, cc2_v: 0, dp_v: 0.6, dm_v: 0.6 };
  switch (scenario) {
    case "idle":
      return { t_ms: tMs, vbus_v: 0.004, ibus_a: 0, ...lines, cc1_v: 0.08 };
    case "load-5v-1a":
      return {
        t_ms: tMs,
        vbus_v: 5 + 0.01 * Math.sin(t * 7),
        ibus_a: 1 + 0.02 * Math.sin(t * 3),
        ...lines,
      };
    case "pd-9v-2a":
      return {
        t_ms: tMs,
        vbus_v: 9 + 0.02 * Math.sin(t * 5),
        ibus_a: 2 + 0.05 * Math.sin(t * 2),
        ...lines,
        cc1_v: 1.05,
      };
    case "noisy-negative":
      return {
        t_ms: tMs,
        vbus_v: 5 + 0.05 * Math.sin(t * 31),
        ibus_a: -0.2 + 0.05 * Math.sin(t * 17),
        ...lines,
      };
  }
}

interface Running {
  scenario: Scenario;
  rateSps: number;
  /** Device time of the next sample, ms. */
  nextMs: number;
  /** Fractional samples owed to the next batch. */
  carry: number;
  timer: ReturnType<typeof setInterval>;
  onEvent: (event: BackendEvent) => void;
}

/** One page-lifetime instance; a running session's timer stops on `disconnect()` or
 * when the page goes away (it is a test and demo double, not a resource owner). */
export class MockBackend {
  #session: Running | null = null;

  async listDevices(): Promise<DeviceListing[]> {
    return SCENARIOS.map((name) => ({
      id: `demo:${name}`,
      label: `Demo device — ${name}`,
      model: "KM003C",
      serial: "123456",
      bus: null,
      address: null,
      demo_scenario: name,
    }));
  }

  /** The demo list never changes, so there is nothing to unsubscribe from. */
  async watchDevices(onChange: (devices: DeviceListing[]) => void): Promise<() => void> {
    onChange(await this.listDevices());
    return () => {};
  }

  async connect(
    deviceId: string,
    rateSps: number,
    onEvent: (event: BackendEvent) => void,
  ): Promise<void> {
    if (this.#session) throw usage("A meter is already connected. Disconnect it first.");
    const scenario = scenarioOf(deviceId);
    if (!scenario) throw usage("That meter is no longer attached.");
    onEvent({
      type: "status",
      data: {
        state: "connected",
        model: "KM003C",
        firmware_version: "2.0.6",
        serial: "123456",
        rate_sps: rateSps,
        rate_locked: false,
        session_id: 1,
      },
    });
    const session: Running = {
      scenario,
      rateSps,
      nextMs: 0,
      carry: 0,
      onEvent,
      timer: setInterval(() => this.#tick(session), BATCH_MS),
    };
    this.#session = session;
  }

  #tick(s: Running): void {
    s.carry += (s.rateSps * BATCH_MS) / 1000;
    const n = Math.floor(s.carry);
    s.carry -= n;
    const step = 1000 / s.rateSps;
    const samples: Sample[] = [];
    for (let i = 0; i < n; i++) {
      samples.push(sampleAt(s.scenario, Math.round(s.nextMs)));
      s.nextMs += step;
    }
    const tempC = s.rateSps === 1000 ? null : 30.5 + 0.2 * Math.sin(s.nextMs / 60_000);
    s.onEvent({ type: "batch", data: { samples, temp_c: tempC } });
  }

  async disconnect(): Promise<void> {
    const s = this.#session;
    if (!s) throw usage("No meter is connected.");
    clearInterval(s.timer);
    this.#session = null;
    s.onEvent({ type: "status", data: { state: "disconnected", error: null } });
  }

  async setRate(rateSps: number): Promise<void> {
    const s = this.#session;
    if (!s) throw usage("No meter is connected.");
    s.rateSps = rateSps;
    s.carry = 0;
    s.onEvent({ type: "status", data: { state: "rate", rate_sps: rateSps, rate_locked: false } });
  }

  async appInfo(): Promise<AppInfo> {
    return { appVersion: "0.1.0 (mock backend)", protocolVersion: "0.1.0" };
  }
}
