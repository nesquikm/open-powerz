/**
 * A scripted stand-in for the Tauri backend, so the web build runs in a
 * plain browser (`VITE_BACKEND=mock`): the Playwright UI suite and the
 * visual checks drive it. Its scenarios follow the simulator's shapes in
 * `crates/powerz-sim`; like firmware 2.0.6, it reports no temperature
 * while the queue runs at 1000 SPS.
 */

import type {
  AppInfo,
  BackendEvent,
  BucketsFor,
  DeviceListing,
  ExportProgress,
  ExportResult,
  MinMaxAvg,
  RecordingSummary,
  Sample,
  Series,
  SeriesBuckets,
  StatsSnapshot,
  UiError,
} from "./backend";

const SCENARIOS = ["idle", "load-5v-1a", "pd-9v-2a", "noisy-negative"] as const;
type Scenario = (typeof SCENARIOS)[number];

/** Batches go out every 25 ms: 40 Hz, inside the backend's 20–60 Hz band. */
const BATCH_MS = 25;

/** Recording progress goes out every 500 ms, like the backend's. */
const PROGRESS_MS = 500;

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

/** Running min / max / sum of one quantity. */
class RunningMinMax {
  min = Infinity;
  max = -Infinity;
  sum = 0;

  push(x: number): void {
    this.min = Math.min(this.min, x);
    this.max = Math.max(this.max, x);
    this.sum += x;
  }

  snapshot(n: number): MinMaxAvg | null {
    return n > 0 ? { min: this.min, max: this.max, avg: this.sum / n } : null;
  }
}

/** Statistics over the samples since the last reset, like `src-tauri/src/stats.rs`:
 * charge and energy by the trapezoid rule, never across a reset. */
class MockStats {
  #first: number | null = null;
  #last: Sample | null = null;
  #samples = 0;
  #chargeAh = 0;
  #energyWh = 0;
  #vbus = new RunningMinMax();
  #ibus = new RunningMinMax();
  #power = new RunningMinMax();

  push(s: Sample): void {
    const prev = this.#last;
    if (prev) {
      const dtH = (s.t_ms - prev.t_ms) / 3_600_000;
      this.#chargeAh += ((prev.ibus_a + s.ibus_a) / 2) * dtH;
      this.#energyWh += ((prev.vbus_v * prev.ibus_a + s.vbus_v * s.ibus_a) / 2) * dtH;
    }
    this.#first ??= s.t_ms;
    this.#last = s;
    this.#samples += 1;
    this.#vbus.push(s.vbus_v);
    this.#ibus.push(s.ibus_a);
    this.#power.push(s.vbus_v * s.ibus_a);
  }

  snapshot(): StatsSnapshot {
    const n = this.#samples;
    return {
      elapsed_ms: this.#first === null || !this.#last ? 0 : this.#last.t_ms - this.#first,
      samples: n,
      charge_ah: this.#chargeAh,
      energy_wh: this.#energyWh,
      vbus: this.#vbus.snapshot(n),
      ibus: this.#ibus.snapshot(n),
      power: this.#power.snapshot(n),
    };
  }
}

const SERIES_FIELD: Record<Series, keyof Sample> = {
  vbus: "vbus_v",
  ibus: "ibus_a",
  dp: "dp_v",
  dm: "dm_v",
  cc1: "cc1_v",
  cc2: "cc2_v",
};

/** Min / max buckets of `history` over `t0..=t1`, at most `buckets` per series. */
function bucketsOf(
  history: Sample[],
  series: readonly Series[],
  t0: number,
  t1: number,
  buckets: number,
): SeriesBuckets[] {
  const inRange = history.filter((s) => s.t_ms >= t0 && s.t_ms <= t1);
  const per = buckets > 0 ? Math.max(1, Math.ceil(inRange.length / buckets)) : 0;
  return series.map((name) => {
    const field = SERIES_FIELD[name];
    const out: SeriesBuckets = { series: name, buckets: [] };
    if (per === 0) return out;
    for (let i = 0; i < inRange.length; i += per) {
      const chunk = inRange.slice(i, i + per);
      const head = chunk[0];
      if (!head) break;
      const values = chunk.map((s) => s[field]);
      out.buckets.push({ t_ms: head.t_ms, min: Math.min(...values), max: Math.max(...values) });
    }
    return out;
  });
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
  /** Samples streamed so far (what an export writes). */
  streamed: number;
  /** The open recording, if any. */
  recording: MockRecording | null;
  /** Every sample since the start or the last clear (what the chart zooms into). */
  history: Sample[];
  stats: MockStats;
}

interface MockRecording {
  path: string;
  rows: number;
  elapsedMs: number;
  sinceProgressMs: number;
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
      streamed: 0,
      recording: null,
      history: [],
      stats: new MockStats(),
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
      const sample = sampleAt(s.scenario, Math.round(s.nextMs));
      samples.push(sample);
      s.history.push(sample);
      s.stats.push(sample);
      s.nextMs += step;
    }
    const tempC = s.rateSps === 1000 ? null : 30.5 + 0.2 * Math.sin(s.nextMs / 60_000);
    s.onEvent({ type: "batch", data: { samples, temp_c: tempC, stats: s.stats.snapshot() } });
    s.streamed += n;
    const rec = s.recording;
    if (rec) {
      rec.rows += n;
      rec.elapsedMs += BATCH_MS;
      rec.sinceProgressMs += BATCH_MS;
      if (rec.sinceProgressMs >= PROGRESS_MS) {
        rec.sinceProgressMs = 0;
        s.onEvent({
          type: "recording",
          data: { state: "progress", rows: rec.rows, elapsed_ms: rec.elapsedMs },
        });
      }
    }
  }

  /** The open session; refuses like the backend when none is. */
  #open(): Running {
    const s = this.#session;
    if (!s) throw usage("No meter is connected.");
    return s;
  }

  async disconnect(): Promise<void> {
    const s = this.#open();
    clearInterval(s.timer);
    this.#session = null;
    if (s.recording) {
      const rows = s.recording.rows;
      s.recording = null;
      s.onEvent({
        type: "recording",
        data: { state: "stopped", rows, gaps: [], disconnected: true },
      });
    }
    s.onEvent({ type: "status", data: { state: "disconnected", error: null } });
  }

  async setRate(rateSps: number): Promise<void> {
    const s = this.#open();
    s.rateSps = rateSps;
    s.carry = 0;
    s.onEvent({ type: "status", data: { state: "rate", rate_sps: rateSps, rate_locked: false } });
  }

  /** There is no save dialog in the browser: the proposed name is the "path". */
  async startRecording(suggestedName: string): Promise<string | null> {
    const s = this.#open();
    if (s.recording) throw usage("A recording is already running.");
    s.recording = { path: suggestedName, rows: 0, elapsedMs: 0, sinceProgressMs: 0 };
    s.onEvent({ type: "recording", data: { state: "started" } });
    return suggestedName;
  }

  async stopRecording(): Promise<RecordingSummary> {
    const s = this.#open();
    const rec = s.recording;
    if (!rec) throw usage("Nothing is being recorded.");
    s.recording = null;
    return { rows: rec.rows, gaps: [], disconnected: false };
  }

  /** Exports every sample streamed so far, finishing on the next timer tick. */
  async exportSession(
    suggestedName: string,
    onProgress: (progress: ExportProgress) => void,
  ): Promise<ExportResult | null> {
    const s = this.#open();
    const rows = s.streamed;
    return new Promise((resolve) =>
      setTimeout(() => {
        onProgress({ done: rows, total: rows });
        resolve({ path: suggestedName, rows });
      }, 0),
    );
  }

  async stats(): Promise<StatsSnapshot> {
    const s = this.#open();
    return s.stats.snapshot();
  }

  async historyRange<const S extends readonly Series[]>(
    series: S,
    t0Ms: number,
    t1Ms: number,
    buckets: number,
  ): Promise<BucketsFor<S>> {
    const s = this.#open();
    // `bucketsOf` maps `series` one to one, so the tuple shape holds.
    return bucketsOf(s.history, series, t0Ms, t1Ms, buckets) as BucketsFor<S>;
  }

  /** Zeroes the stats; they count again from the next sample. The history stays. */
  async resetStats(): Promise<void> {
    const s = this.#open();
    s.stats = new MockStats();
  }

  /** Discards the history; the stream and any recording go on. */
  async clearHistory(): Promise<void> {
    const s = this.#open();
    s.history = [];
  }

  async appInfo(): Promise<AppInfo> {
    return { appVersion: "0.1.0 (mock backend)", protocolVersion: "0.1.0" };
  }
}
