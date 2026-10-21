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
  Gap,
  HistoryGap,
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

/** Writes the scenario's reading at device time `tMs` into `out` (so a large seed can
 * reuse one object instead of allocating a sample per point). */
function writeSample(scenario: Scenario, tMs: number, out: Sample): Sample {
  const t = tMs / 1000;
  out.t_ms = tMs;
  out.cc1_v = 1.68;
  out.cc2_v = 0;
  out.dp_v = 0.6;
  out.dm_v = 0.6;
  switch (scenario) {
    case "idle":
      out.vbus_v = 0.004;
      out.ibus_a = 0;
      out.cc1_v = 0.08;
      break;
    case "load-5v-1a":
      out.vbus_v = 5 + 0.01 * Math.sin(t * 7);
      out.ibus_a = 1 + 0.02 * Math.sin(t * 3);
      break;
    case "pd-9v-2a":
      out.vbus_v = 9 + 0.02 * Math.sin(t * 5);
      out.ibus_a = 2 + 0.05 * Math.sin(t * 2);
      out.cc1_v = 1.05;
      break;
    case "noisy-negative":
      out.vbus_v = 5 + 0.05 * Math.sin(t * 31);
      out.ibus_a = -0.2 + 0.05 * Math.sin(t * 17);
      break;
  }
  return out;
}

/** The scenario's reading at device time `tMs`. */
function sampleAt(scenario: Scenario, tMs: number): Sample {
  return writeSample(scenario, tMs, {
    t_ms: 0,
    vbus_v: 0,
    ibus_a: 0,
    cc1_v: 0,
    cc2_v: 0,
    dp_v: 0,
    dm_v: 0,
  });
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

const FIELDS = Object.keys(SERIES_FIELD) as Series[];

/** Spare room left after a synthetic seed, so live samples append without a copy. */
const SEED_HEADROOM = 1 << 20;

/** The session history as columns (sample times plus one column per series), so a
 * range is found by binary search and bucketed without building sample objects:
 * a pan over a 10-million-sample history stays well inside a frame (AC-AH2AEM.4). */
class HistoryColumns {
  length = 0;
  #t = new Float64Array(0);
  /** Float32 values (plenty for a demo double) keep a 10M seed near 320 MB. */
  #cols = {} as Record<Series, Float32Array>;

  constructor(capacity = 1024) {
    this.#alloc(capacity);
  }

  #alloc(capacity: number): void {
    const t = new Float64Array(capacity);
    t.set(this.#t.subarray(0, this.length));
    this.#t = t;
    for (const name of FIELDS) {
      const col = new Float32Array(capacity);
      const old = this.#cols[name] as Float32Array | undefined;
      if (old) col.set(old.subarray(0, this.length));
      this.#cols[name] = col;
    }
  }

  push(s: Sample): void {
    if (this.length === this.#t.length) this.#alloc(Math.max(1024, this.#t.length * 2));
    const i = this.length++;
    this.#t[i] = s.t_ms;
    for (const name of FIELDS) this.#cols[name][i] = s[SERIES_FIELD[name]] as number;
  }

  /** Appends `n` scenario samples, one per millisecond from 0, straight into the
   * columns (no sample object per point). */
  fillSynthetic(scenario: Scenario, n: number): void {
    if (this.length + n > this.#t.length) this.#alloc(this.length + n + SEED_HEADROOM);
    const s = sampleAt(scenario, 0);
    for (let t = 0; t < n; t++) {
      writeSample(scenario, t, s);
      const i = this.length++;
      this.#t[i] = t;
      for (const name of FIELDS) this.#cols[name][i] = s[SERIES_FIELD[name]] as number;
    }
  }

  /** The first index whose time is `>= t` (or `> t` when `after`). */
  #search(t: number, after: boolean): number {
    let lo = 0;
    let hi = this.length;
    while (lo < hi) {
      const mid = (lo + hi) >>> 1;
      const v = this.#t[mid] as number;
      if (v < t || (after && v === t)) lo = mid + 1;
      else hi = mid;
    }
    return lo;
  }

  /** Min / max buckets over `t0..=t1`, at most `buckets` per series. */
  buckets(series: readonly Series[], t0: number, t1: number, buckets: number): SeriesBuckets[] {
    const from = this.#search(t0, false);
    const to = this.#search(t1, true);
    const n = Math.max(0, to - from);
    const per = buckets > 0 ? Math.max(1, Math.ceil(n / buckets)) : 0;
    return series.map((name) => {
      const out: SeriesBuckets = { series: name, buckets: [] };
      if (per === 0) return out;
      const col = this.#cols[name];
      for (let i = from; i < to; i += per) {
        const end = Math.min(to, i + per);
        let min = Infinity;
        let max = -Infinity;
        for (let j = i; j < end; j++) {
          const v = col[j] as number;
          if (v < min) min = v;
          if (v > max) max = v;
        }
        out.buckets.push({ t_ms: this.#t[i] as number, min, max });
      }
      return out;
    });
  }
}

/** The page URL's query string (`location.search`), if there is a page. */
function pageSearch(): string | undefined {
  return (globalThis as { location?: { search?: string } }).location?.search;
}

/** Parameter `name` of the query string `search`, or null when absent. */
function urlParam(search: string | undefined, name: string): string | null {
  return search ? new URLSearchParams(search).get(name) : null;
}

/** The largest synthetic seed (a 20M seed is already about 640 MB of columns). */
export const MAX_SYNTHETIC_SAMPLES = 20_000_000;

/** `?synthetic=<n>` in the page URL seeds an `n`-sample history (at most
 * {@link MAX_SYNTHETIC_SAMPLES}) on connect, for the Playwright pan-performance check
 * (AC-AH2AEM.4); dev and mock builds only, and the device list is unchanged. */
export function syntheticFromUrl(search: string | undefined, dev: boolean): number | null {
  const raw = dev ? urlParam(search, "synthetic") : null;
  if (raw === null) return null;
  const n = Number(raw);
  return Number.isInteger(n) && n > 0 ? Math.min(n, MAX_SYNTHETIC_SAMPLES) : null;
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
  history: HistoryColumns;
  /** The gaps between the history's runs (`outage` only). */
  gaps: HistoryGap[];
  /** The outage gap the backend announces before the first samples after it. */
  pendingGap: Gap | null;
  stats: MockStats;
}

interface MockRecording {
  path: string;
  rows: number;
  elapsedMs: number;
  sinceProgressMs: number;
}

/** Scripted backend behaviours chosen with `?mock=<name>` in the page URL; the device
 * list stays the same in every one. */
export const MOCK_MODES = ["permission-denied", "rate-locked", "slow-export", "outage"] as const;
export type MockMode = (typeof MOCK_MODES)[number];

/** The `?mock=` mode of the page URL, or null for the normal demo behaviour. */
export function mockModeFromUrl(search: string | undefined): MockMode | null {
  const name = urlParam(search, "mock");
  return MOCK_MODES.find((m) => m === name) ?? null;
}

/** `outage`: the meter has been up for a while (its clock is far from zero), streams
 * `beforeMs`, is gone for `gapMs` (unplugged, then connected again), and the live stream
 * goes on after that; the history holds both runs and reports the gap between them. */
export const MOCK_OUTAGE = { startMs: 30_400_000, beforeMs: 20_000, gapMs: 2_400_000 } as const;

/** How long a `slow-export` export stays half done, so the UI suite can see its progress. */
const SLOW_EXPORT_MS = 30_000;

/** The meter's own rate when authentication fails (`rate-locked`). */
const LOCKED_RATE_SPS = 50;

/** The Linux permission failure of `src-tauri/src/ui_error.rs` (the source of truth for the
 * wording; `mockBackend.test.ts` checks this copy against it, and the UI suite reads it). */
export const LINUX_PERMISSION_DENIED: Readonly<UiError> = {
  kind: "permission_denied",
  message: "The system did not allow the app to open the meter.",
  hint:
    "Install the udev rule: sudo cp packaging/linux/60-powerz.rules /etc/udev/rules.d/, then run " +
    "sudo udevadm control --reload-rules and sudo udevadm trigger, and replug the meter " +
    "(see docs/linux.md).",
};

/** One page-lifetime instance; a running session's timer stops on `disconnect()` or
 * when the page goes away (it is a test and demo double, not a resource owner). */
export class MockBackend {
  #session: Running | null = null;
  #mode: MockMode | null;

  /** `mode` defaults to the page URL's `?mock=` parameter. */
  constructor(mode: MockMode | null = mockModeFromUrl(pageSearch())) {
    this.#mode = mode;
  }

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
    if (this.#mode === "permission-denied") throw { ...LINUX_PERMISSION_DENIED };
    const locked = this.#mode === "rate-locked";
    // Like the worker: a locked session's rate is unknown until measured.
    onEvent({
      type: "status",
      data: {
        state: "connected",
        model: "KM003C",
        firmware_version: "2.0.6",
        serial: "123456",
        rate_sps: locked ? null : rateSps,
        rate_locked: locked,
        session_id: 1,
      },
    });
    if (locked) {
      onEvent({
        type: "status",
        data: { state: "rate", rate_sps: LOCKED_RATE_SPS, rate_locked: true },
      });
    }
    const session: Running = {
      scenario,
      rateSps: locked ? LOCKED_RATE_SPS : rateSps,
      nextMs: 0,
      carry: 0,
      onEvent,
      streamed: 0,
      recording: null,
      history: new HistoryColumns(),
      gaps: [],
      pendingGap: null,
      stats: new MockStats(),
      timer: setInterval(() => this.#tick(session), BATCH_MS),
    };
    this.#session = session;
    if (this.#mode === "outage") this.#seedOutage(session);
    // The mock production build (the Playwright suite) honours it too.
    const synthetic = syntheticFromUrl(
      pageSearch(),
      import.meta.env.DEV || import.meta.env.VITE_BACKEND === "mock",
    );
    if (synthetic !== null) await this.seedSyntheticHistory(synthetic);
  }

  /** `outage`: the run before the outage, then the live stream after it. */
  #seedOutage(s: Running): void {
    const step = 1000 / s.rateSps;
    const end = MOCK_OUTAGE.startMs + MOCK_OUTAGE.beforeMs;
    let last: number = MOCK_OUTAGE.startMs;
    for (let t = MOCK_OUTAGE.startMs; t < end; t += step) {
      last = Math.round(t);
      s.history.push(sampleAt(s.scenario, last));
    }
    s.nextMs = end + MOCK_OUTAGE.gapMs;
    const gap = { from_ms: last, to_ms: s.nextMs };
    s.gaps.push(gap);
    s.pendingGap = { kind: "outage", ...gap };
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
    if (s.pendingGap !== null && n > 0) {
      s.onEvent({ type: "gap", data: s.pendingGap });
      s.pendingGap = null;
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
    if (this.#mode === "rate-locked") {
      throw usage("Changing the rate needs authentication, which the meter refused.");
    }
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

  /** Exports every sample streamed so far, finishing on the next timer tick (`slow-export`:
   * reports half the rows at once and finishes after {@link SLOW_EXPORT_MS}). */
  async exportSession(
    suggestedName: string,
    onProgress: (progress: ExportProgress) => void,
  ): Promise<ExportResult | null> {
    const s = this.#open();
    const rows = s.streamed;
    const slow = this.#mode === "slow-export";
    if (slow) onProgress({ done: Math.floor(rows / 2), total: rows });
    return new Promise((resolve) =>
      setTimeout(
        () => {
          onProgress({ done: rows, total: rows });
          resolve({ path: suggestedName, rows });
        },
        slow ? SLOW_EXPORT_MS : 0,
      ),
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
    // `HistoryColumns.buckets` maps `series` one to one, so the tuple shape holds.
    return s.history.buckets(series, t0Ms, t1Ms, buckets) as BucketsFor<S>;
  }

  async historyGaps(t0Ms: number, t1Ms: number): Promise<HistoryGap[]> {
    const s = this.#open();
    return s.gaps.filter((g) => g.to_ms > t0Ms && g.from_ms < t1Ms).map((g) => ({ ...g }));
  }

  /** Replaces the history with `n` (at most {@link MAX_SYNTHETIC_SAMPLES}) scenario
   * samples, one per millisecond from 0, and moves the live stream past them: a large
   * history for the pan check (AC-AH2AEM.4). */
  async seedSyntheticHistory(n: number): Promise<void> {
    const s = this.#open();
    const count = Math.max(0, Math.min(Math.floor(n), MAX_SYNTHETIC_SAMPLES));
    const history = new HistoryColumns(count + SEED_HEADROOM);
    history.fillSynthetic(s.scenario, count);
    s.history = history;
    s.nextMs = Math.max(s.nextMs, count);
  }

  /** Zeroes the stats; they count again from the next sample. The history stays. */
  async resetStats(): Promise<void> {
    const s = this.#open();
    s.stats = new MockStats();
  }

  /** Discards the history; the stream and any recording go on. */
  async clearHistory(): Promise<void> {
    const s = this.#open();
    s.history = new HistoryColumns();
    s.gaps = [];
  }

  async appInfo(): Promise<AppInfo> {
    return { appVersion: "0.1.0 (mock backend)", protocolVersion: "0.1.0" };
  }
}
