/**
 * FR AXHK2T (AC-AXHK2T.2): the demo backend answers the ruler from its raw samples, as
 * the Rust `range_stats` does: charge and energy by the trapezoid rule over the samples
 * with `t0 <= t <= t1`, averages of those samples, `null` when none is in range. Checked
 * against an independent sum over the scenario's own formula, within 0.1%.
 */

import { afterEach, describe, expect, it } from "vitest";
import type { RangeStats } from "./backend";
import { MOCK_RECORDING_FILE, MockBackend } from "./mockBackend";

/** The `load-5v-1a` demo reading at `tMs` (the scenario's formula). */
function load(tMs: number): { v: number; i: number } {
  const t = tMs / 1000;
  return { v: 5 + 0.01 * Math.sin(t * 7), i: 1 + 0.02 * Math.sin(t * 3) };
}

/** The reference over whole-ms samples `t0..=t1` of `load`. */
function reference(t0: number, t1: number): RangeStats {
  let chargeAh = 0;
  let energyWh = 0;
  let sv = 0;
  let si = 0;
  let sp = 0;
  let n = 0;
  let prev: { t: number; v: number; i: number } | null = null;
  for (let t = Math.ceil(t0); t <= Math.floor(t1); t++) {
    const { v, i } = load(t);
    if (prev) {
      const h = (t - prev.t) / 3_600_000;
      chargeAh += ((prev.i + i) / 2) * h;
      energyWh += ((prev.v * prev.i + v * i) / 2) * h;
    }
    prev = { t, v, i };
    sv += v;
    si += i;
    sp += v * i;
    n++;
  }
  return {
    samples: n,
    charge_ah: chargeAh,
    energy_wh: energyWh,
    vbus_avg_v: sv / n,
    ibus_avg_a: si / n,
    power_avg_w: sp / n,
  };
}

function expectWithin(got: RangeStats | null, want: RangeStats, rel = 1e-3): void {
  expect(got).not.toBeNull();
  if (got === null) return;
  expect(got.samples).toBe(want.samples);
  for (const key of [
    "charge_ah",
    "energy_wh",
    "vbus_avg_v",
    "ibus_avg_a",
    "power_avg_w",
  ] as const) {
    expect(Math.abs(got[key] - want[key]) / Math.abs(want[key]), key).toBeLessThanOrEqual(rel);
  }
}

let backend: MockBackend | null = null;
afterEach(async () => {
  await backend?.disconnect().catch(() => {});
  backend = null;
});

describe("MockBackend ruler stats over an opened recording (AC-AXHK2T.2)", () => {
  it("match the raw samples of the file within 0.1%", async () => {
    backend = new MockBackend(null);
    await backend.openRecordingFile(() => {});
    for (const [t0, t1] of [
      [0, MOCK_RECORDING_FILE.rows - 1],
      [1_234.4, 5_678.9],
      [4_000, 8_000],
    ] as const) {
      expectWithin(await backend.recordingFileRangeStats(t0, t1), reference(t0, t1));
    }
  });

  it("take the times in either order", async () => {
    backend = new MockBackend(null);
    await backend.openRecordingFile(() => {});
    expect(await backend.recordingFileRangeStats(8_000, 4_000)).toEqual(
      await backend.recordingFileRangeStats(4_000, 8_000),
    );
  });

  it("answer null past the file's end, and refuse with no file open", async () => {
    backend = new MockBackend(null);
    await backend.openRecordingFile(() => {});
    await expect(backend.recordingFileRangeStats(50_000, 60_000)).resolves.toBeNull();
    await backend.closeRecordingFile();
    await expect(backend.recordingFileRangeStats(0, 1_000)).rejects.toMatchObject({
      kind: "usage",
    });
  });
});

describe("MockBackend ruler stats over a meter's session (AC-AXHK2T.2)", () => {
  it("match the raw samples of the history within 0.1%", async () => {
    backend = new MockBackend();
    await backend.connect("demo:load-5v-1a", 1000, () => {});
    await backend.seedSyntheticHistory(20_000);
    expectWithin(await backend.rangeStats(2_500, 17_500), reference(2_500, 17_500));
  });

  it("answer null for a range with no sample", async () => {
    backend = new MockBackend();
    await backend.connect("demo:load-5v-1a", 1000, () => {});
    await backend.seedSyntheticHistory(1_000);
    await backend.clearHistory();
    await expect(backend.rangeStats(0, 500)).resolves.toBeNull();
  });

  it("each meter answers from its own history", async () => {
    backend = new MockBackend();
    await backend.connect("demo:load-5v-1a", 1000, () => {});
    await backend.seedSyntheticHistory(5_000);
    const viaMeter = await backend.meter("demo:load-5v-1a").rangeStats(1_000, 4_000);
    expectWithin(viaMeter, reference(1_000, 4_000));
  });
});
