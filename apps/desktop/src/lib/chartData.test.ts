import { describe, expect, it } from "vitest";
import type { Gap, Sample, SeriesBuckets } from "./backend";
import { CHART_SERIES, defaultVisibility, toggleSeries } from "./chartSeries";
import {
  LIVE_CAPACITY,
  LIVE_WINDOWS,
  GapList,
  LiveBuffer,
  MAX_GAPS,
  bucketsToFrame,
  hoverReadout,
  liveRange,
  steppedRange,
  toAlignedData,
  type ChartFrame,
} from "./chartData";

function sample(t: number, vbus = 5 + t / 1000, ibus = 1): Sample {
  return { t_ms: t, vbus_v: vbus, ibus_a: ibus, cc1_v: 1.68, cc2_v: 0, dp_v: 0.6, dm_v: 0.6 };
}

function samples(from: number, to: number, step = 1): Sample[] {
  const out: Sample[] = [];
  for (let t = from; t <= to; t += step) out.push(sample(t));
  return out;
}

function nonNull(values: (number | null)[]): number[] {
  return values.filter((v): v is number => v !== null);
}

describe("LiveBuffer ring (AC-AH2AEM.2)", () => {
  it("holds at least 10 minutes at 1000 SPS by default", () => {
    expect(LIVE_CAPACITY).toBeGreaterThanOrEqual(600_000);
  });

  it("keeps the newest samples and drops the oldest when full", () => {
    const buf = new LiveBuffer(100);
    buf.push(samples(0, 149));
    expect(buf.length).toBe(100);
    expect(buf.firstMs).toBe(50);
    expect(buf.lastMs).toBe(149);
  });

  it("is empty before the first batch", () => {
    const buf = new LiveBuffer(10);
    expect(buf.length).toBe(0);
    expect(buf.firstMs).toBeNull();
    expect(buf.lastMs).toBeNull();
    expect(liveRange("10s", buf)).toBeNull();
  });

  it("returns the raw samples of a window that fits the plot", () => {
    const buf = new LiveBuffer(1000);
    buf.push(samples(0, 99));
    const frame = buf.window(10, 19, 800);
    expect(frame.decimated).toBe(false);
    expect(frame.x).toEqual([10, 11, 12, 13, 14, 15, 16, 17, 18, 19]);
    expect(frame.y.vbus[0]).toBeCloseTo(5.01, 6);
    expect(frame.y.ibus.every((v) => v === 1)).toBe(true);
    expect(frame.y.cc1.every((v) => v === 1.68)).toBe(true);
  });

  it("derives power as VBUS × IBUS", () => {
    const buf = new LiveBuffer(10);
    buf.push([sample(0, 9, 2), sample(1, 5, 0.5)]);
    const frame = buf.window(0, 1, 100);
    expect(frame.y.power[0]).toBeCloseTo(18, 9);
    expect(frame.y.power[1]).toBeCloseTo(2.5, 9);
  });

  it("reduces a dense window to at most two points per pixel, keeping the extremes", () => {
    const buf = new LiveBuffer(20_000);
    const data = samples(0, 9_999).map((s, i) => ({
      ...s,
      vbus_v: 5 + Math.sin(i / 7) + (i === 4321 ? 3 : 0) - (i === 7777 ? 4 : 0),
    }));
    buf.push(data);
    const frame = buf.window(0, 9_999, 200);
    expect(frame.decimated).toBe(true);
    expect(frame.x.length).toBeLessThanOrEqual(400);
    expect(frame.x.length).toBeGreaterThan(100);
    const vbus = nonNull(frame.y.vbus);
    expect(Math.max(...vbus)).toBeCloseTo(Math.max(...data.map((s) => s.vbus_v)), 9);
    expect(Math.min(...vbus)).toBeCloseTo(Math.min(...data.map((s) => s.vbus_v)), 9);
    for (let i = 1; i < frame.x.length; i++) {
      expect(frame.x[i]).toBeGreaterThanOrEqual(frame.x[i - 1] ?? -Infinity);
    }
  });

  it("scrolls the live window as batches arrive", () => {
    const buf = new LiveBuffer(LIVE_CAPACITY);
    buf.push(samples(0, 20_000, 1));
    expect(liveRange("10s", buf)).toEqual([10_000, 20_000]);
    buf.push(samples(20_025, 20_025));
    expect(liveRange("10s", buf)).toEqual([10_025, 20_025]);
  });
});

describe("live windows (AC-AH2AEM.2)", () => {
  it("offers 10 s, 1 min, 10 min and the whole session", () => {
    expect(LIVE_WINDOWS.map((w) => w.ms)).toEqual([10_000, 60_000, 600_000, null]);
    expect(LIVE_WINDOWS.map((w) => w.id)).toEqual(["10s", "1m", "10m", "all"]);
    for (const w of LIVE_WINDOWS) expect(w.label).toBeTypeOf("string");
  });

  it("maps each window to [last − span, last], and the whole session to [first, last]", () => {
    const buf = new LiveBuffer(LIVE_CAPACITY);
    buf.push(samples(1_000, 900_000, 100));
    expect(liveRange("10s", buf)).toEqual([890_000, 900_000]);
    expect(liveRange("1m", buf)).toEqual([840_000, 900_000]);
    expect(liveRange("10m", buf)).toEqual([300_000, 900_000]);
    expect(liveRange("all", buf)).toEqual([1_000, 900_000]);
  });
});

describe("history buckets to series (AC-AH2AEM.3, AC-AH2AEM.6)", () => {
  const result: SeriesBuckets[] = [
    {
      series: "vbus",
      buckets: [
        { t_ms: 0, min: 4.9, max: 5.1 },
        { t_ms: 100, min: 4.8, max: 5.2 },
        { t_ms: 200, min: 5.0, max: 5.3 },
      ],
    },
    {
      series: "ibus",
      buckets: [
        { t_ms: 0, min: 0.9, max: 1.1 },
        { t_ms: 100, min: 0.95, max: 1.05 },
        { t_ms: 200, min: 1.0, max: 1.2 },
      ],
    },
  ];

  it("keeps every bucket's min and max in the drawn line", () => {
    const frame = bucketsToFrame(result);
    expect(frame.decimated).toBe(true);
    expect(frame.x.length).toBeGreaterThanOrEqual(3);
    expect(frame.x.length).toBeLessThanOrEqual(6);
    const vbus = nonNull(frame.y.vbus);
    for (const b of result[0]?.buckets ?? []) {
      expect(vbus).toContain(b.min);
      expect(vbus).toContain(b.max);
    }
    expect(frame.x[0]).toBe(0);
    expect(frame.x.at(-1)).toBe(200);
  });

  it("leaves series the answer does not carry empty", () => {
    const frame = bucketsToFrame(result);
    expect(frame.y.cc2.every((v) => v === null)).toBe(true);
    expect(frame.y.cc2).toHaveLength(frame.x.length);
  });

  it("produces uPlot aligned data: x first, then one array per series in CHART_SERIES order", () => {
    const frame = bucketsToFrame(result);
    const data = toAlignedData(frame);
    expect(data).toHaveLength(CHART_SERIES.length + 1);
    expect(data[0]).toEqual(frame.x);
    CHART_SERIES.forEach((s, i) => expect(data[i + 1]).toEqual(frame.y[s.key]));
  });
});

describe("hover readout (AC-AH2AEM.6)", () => {
  function rawFrame(): ChartFrame {
    const buf = new LiveBuffer(100);
    buf.push([sample(10, 5.0), sample(20, 5.5), sample(30, 6.0)]);
    return buf.window(0, 40, 800);
  }

  it("shows the time and value of the nearest sample for each visible series", () => {
    const readout = hoverReadout(rawFrame(), 23, defaultVisibility());
    expect(readout?.t_ms).toBe(20);
    const vbus = readout?.values.find((v) => v.key === "vbus");
    expect(vbus).toEqual({ key: "vbus", kind: "value", value: 5.5 });
    const ibus = readout?.values.find((v) => v.key === "ibus");
    expect(ibus).toEqual({ key: "ibus", kind: "value", value: 1 });
  });

  it("lists only the visible series", () => {
    const hidden = toggleSeries(defaultVisibility(), "cc2");
    const keys = hoverReadout(rawFrame(), 12, hidden)?.values.map((v) => v.key) ?? [];
    expect(keys).not.toContain("cc2");
    expect(keys).not.toContain("power"); // hidden by default
    expect(keys).toContain("vbus");
    const withPower = hoverReadout(rawFrame(), 12, toggleSeries(hidden, "power"));
    expect(withPower?.values.find((v) => v.key === "power")).toEqual({
      key: "power",
      kind: "value",
      value: 5,
    });
  });

  it("shows the min–max range when the view is decimated", () => {
    const frame = bucketsToFrame([
      {
        series: "vbus",
        buckets: [
          { t_ms: 0, min: 4.9, max: 5.1 },
          { t_ms: 100, min: 4.8, max: 5.2 },
        ],
      },
    ]);
    const readout = hoverReadout(frame, 95, defaultVisibility());
    expect(readout?.t_ms).toBe(100);
    expect(readout?.values.find((v) => v.key === "vbus")).toEqual({
      key: "vbus",
      kind: "range",
      min: 4.8,
      max: 5.2,
    });
  });

  it("shows a single value for a bucket whose min equals its max", () => {
    const frame = bucketsToFrame([{ series: "vbus", buckets: [{ t_ms: 0, min: 5, max: 5 }] }]);
    expect(hoverReadout(frame, 3, defaultVisibility())?.values).toContainEqual({
      key: "vbus",
      kind: "value",
      value: 5,
    });
  });

  it("returns nothing over an empty chart", () => {
    const buf = new LiveBuffer(10);
    expect(hoverReadout(buf.window(0, 100, 100), 50, defaultVisibility())).toBeNull();
  });
});

describe("gaps (AC-AH2AEM.7)", () => {
  function hasBreakBetween(frame: ChartFrame, key: "vbus" | "ibus", a: number, b: number) {
    const xs = frame.x;
    const ys = frame.y[key];
    const inside = xs.map((x, i) => ({ x, y: ys[i] })).filter((p) => p.x > a && p.x < b);
    return inside.some((p) => p.y === null) && inside.every((p) => p.y === null);
  }

  it("breaks the live line across a stream gap instead of joining the ends", () => {
    const buf = new LiveBuffer(1000);
    buf.push(samples(0, 9));
    buf.markGap({ kind: "stream", from_ms: 10, to_ms: 50 });
    buf.push(samples(50, 59));
    const frame = buf.window(0, 59, 800);
    expect(hasBreakBetween(frame, "vbus", 9, 50)).toBe(true);
    expect(hasBreakBetween(frame, "ibus", 9, 50)).toBe(true);
    expect(nonNull(frame.y.vbus)).toHaveLength(20);
  });

  it("keeps the break when the window is reduced", () => {
    const buf = new LiveBuffer(50_000);
    buf.push(samples(0, 9_999));
    buf.markGap({ kind: "outage", from_ms: 10_000, to_ms: 20_000 });
    buf.push(samples(20_000, 29_999));
    const frame = buf.window(0, 29_999, 100);
    expect(frame.decimated).toBe(true);
    expect(hasBreakBetween(frame, "vbus", 9_999, 20_000)).toBe(true);
  });

  it("does not break a line that has no gap", () => {
    const buf = new LiveBuffer(1000);
    buf.push(samples(0, 59));
    expect(buf.window(0, 59, 800).y.vbus.some((v) => v === null)).toBe(false);
  });

  it("breaks history buckets across a gap", () => {
    const vbus: SeriesBuckets = {
      series: "vbus",
      buckets: [0, 10, 20, 80, 90].map((t) => ({ t_ms: t, min: 5, max: 5.1 })),
    };
    const frame = bucketsToFrame([vbus], [{ kind: "stream", from_ms: 25, to_ms: 75 }]);
    expect(hasBreakBetween(frame, "vbus", 20, 80)).toBe(true);
    const plain = bucketsToFrame([vbus]);
    expect(plain.y.vbus.some((v) => v === null)).toBe(false);
  });
});

describe("gap edges from the backend (AC-AH2AEM.7)", () => {
  // The backend's gap runs from the last sample before it to the first one after it.
  function joined(frame: ChartFrame, key: "vbus", a: number, b: number): boolean {
    const ia = frame.x.lastIndexOf(a);
    const ib = frame.x.indexOf(b);
    if (ia < 0 || ib < 0) return false;
    for (let i = ia + 1; i < ib; i++) if (frame.y[key][i] === null) return false;
    return true;
  }

  it("breaks the raw live line after the sample at from_ms", () => {
    const buf = new LiveBuffer(1000);
    buf.push(samples(0, 9));
    buf.markGap({ kind: "stream", from_ms: 9, to_ms: 50 });
    buf.push(samples(50, 59));
    const frame = buf.window(0, 59, 800);
    expect(frame.decimated).toBe(false);
    expect(joined(frame, "vbus", 9, 50)).toBe(false);
    expect(nonNull(frame.y.vbus)).toHaveLength(20);
    // the sample at from_ms still joins its neighbour before it
    expect(joined(frame, "vbus", 8, 9)).toBe(true);
  });

  it("breaks the decimated live line after the bucket holding from_ms", () => {
    const buf = new LiveBuffer(50_000);
    buf.push(samples(0, 9_999));
    buf.markGap({ kind: "outage", from_ms: 9_999, to_ms: 20_000 });
    buf.push(samples(20_000, 29_999));
    const frame = buf.window(0, 29_999, 100);
    expect(frame.decimated).toBe(true);
    const at = frame.x.lastIndexOf(9_999);
    const after = frame.x.indexOf(20_000);
    expect(at).toBeGreaterThanOrEqual(0);
    expect(after).toBeGreaterThan(at);
    const between = frame.y.vbus.slice(at + 1, after);
    expect(between.length).toBeGreaterThan(0);
    expect(between.every((v) => v === null)).toBe(true);
    // the from_ms sample is drawn before the break, not after it
    expect(frame.y.vbus[at]).not.toBeNull();
  });

  it("breaks history buckets after the bucket at from_ms", () => {
    const vbus: SeriesBuckets = {
      series: "vbus",
      buckets: [0, 10, 20, 80, 90].map((t) => ({ t_ms: t, min: 5, max: 5.1 })),
    };
    const frame = bucketsToFrame([vbus], [{ kind: "stream", from_ms: 20, to_ms: 80 }]);
    expect(joined(frame, "vbus", 20, 80)).toBe(false);
    expect(joined(frame, "vbus", 10, 20)).toBe(true);
    expect(nonNull(frame.y.vbus)).toHaveLength(10);
  });
});

describe("live range start (AC-AH2AEM.2)", () => {
  it("never starts before the session's first sample", () => {
    const buf = new LiveBuffer(1000);
    buf.push(samples(0, 3_000, 100));
    expect(liveRange("10s", buf)).toEqual([0, 3_000]);
    expect(liveRange("1m", buf)).toEqual([0, 3_000]);
  });
});

describe("reduced-motion stepping (AC-AH2AEK.7)", () => {
  it("steps the window so it never ends past the newest sample", () => {
    expect(steppedRange([10_500, 20_500], 10_000, 0)).toEqual([10_000, 20_000]);
    const [, end] = steppedRange([10_999, 20_999], 10_000, 0);
    expect(end).toBeLessThanOrEqual(20_999);
  });

  it("does not start before the first sample", () => {
    expect(steppedRange([500, 10_500], 10_000, 500)[0]).toBeGreaterThanOrEqual(500);
  });

  it("leaves a window shorter than its span (session start clamp) as it is", () => {
    expect(steppedRange([0, 3_000], 10_000, 0)).toEqual([0, 3_000]);
  });
});

describe("non-finite values", () => {
  function bad(t: number, vbus: number): Sample {
    return { ...sample(t), vbus_v: vbus };
  }

  it("draws a raw non-finite value as no point", () => {
    const buf = new LiveBuffer(10);
    buf.push([sample(0), bad(1, NaN), bad(2, Infinity), sample(3)]);
    const frame = buf.window(0, 3, 800);
    expect(frame.y.vbus[1]).toBeNull();
    expect(frame.y.vbus[2]).toBeNull();
    expect(frame.y.power[1]).toBeNull();
    expect(frame.y.ibus[1]).toBe(1);
  });

  it("skips non-finite values in a reduced bucket, and leaves an all-bad bucket empty", () => {
    const buf = new LiveBuffer(10_000);
    const data = samples(0, 999).map((s) =>
      s.t_ms === 500 ? { ...s, vbus_v: Infinity } : s.t_ms >= 900 ? { ...s, vbus_v: NaN } : s,
    );
    buf.push(data);
    const frame = buf.window(0, 999, 10);
    expect(frame.decimated).toBe(true);
    const finite = nonNull(frame.y.vbus);
    expect(finite.every((v) => Number.isFinite(v))).toBe(true);
    expect(Math.max(...finite)).toBeCloseTo(5.899, 6);
    // the last bucket (900..999) holds only NaN
    expect(frame.y.vbus.slice(-2)).toEqual([null, null]);
    expect(nonNull(frame.y.ibus)).toHaveLength(frame.x.length);
  });

  it("treats a NaN plot width as one pixel", () => {
    const buf = new LiveBuffer(10_000);
    buf.push(samples(0, 999));
    const frame = buf.window(0, 999, NaN);
    expect(frame.x.length).toBeGreaterThan(0);
    expect(frame.x.length).toBeLessThanOrEqual(2);
  });

  it("maps non-finite history bucket values to no point", () => {
    const frame = bucketsToFrame([
      { series: "vbus", buckets: [{ t_ms: 0, min: NaN, max: Infinity }] },
    ]);
    expect(frame.y.vbus).toEqual([null, null]);
  });
});

describe("hover readout skips breaks (AC-AH2AEM.6)", () => {
  it("never picks a gap break as the nearest point", () => {
    const buf = new LiveBuffer(100);
    buf.push([sample(0, 5), sample(10, 6)]);
    buf.markGap({ kind: "stream", from_ms: 10, to_ms: 100 });
    buf.push([sample(100, 7), sample(110, 8)]);
    const frame = buf.window(0, 110, 800);
    const readout = hoverReadout(frame, 55, defaultVisibility());
    expect(readout).not.toBeNull();
    expect(readout?.values.length).toBeGreaterThan(0);
    expect([10, 100]).toContain(readout?.t_ms);
  });

  it("skips a decimated break pair too", () => {
    const vbus: SeriesBuckets = {
      series: "vbus",
      buckets: [0, 10, 80, 90].map((t) => ({ t_ms: t, min: 5, max: 5.1 })),
    };
    const frame = bucketsToFrame([vbus], [{ kind: "stream", from_ms: 10, to_ms: 80 }]);
    const readout = hoverReadout(frame, 45, defaultVisibility());
    expect(readout?.values.find((v) => v.key === "vbus")).toBeDefined();
  });
});

describe("GapList", () => {
  const gap = (from: number): Gap => ({ kind: "stream", from_ms: from, to_ms: from + 5 });

  it("returns the gaps starting inside a range, oldest first", () => {
    const list = new GapList();
    list.push(gap(30));
    list.push(gap(10));
    list.push(gap(20));
    expect(list.within(10, 25).map((g) => g.from_ms)).toEqual([10, 20]);
    expect(list.within(31, 100)).toEqual([]);
  });

  it("keeps only the newest gaps once full", () => {
    const list = new GapList(3);
    for (const t of [1, 2, 3, 4, 5]) list.push(gap(t));
    expect(list.length).toBe(3);
    expect(list.within(0, 10).map((g) => g.from_ms)).toEqual([3, 4, 5]);
    expect(MAX_GAPS).toBeGreaterThanOrEqual(1_000);
    expect(MAX_GAPS).toBeLessThanOrEqual(100_000);
  });

  it("drops gaps that start before a time, and clears", () => {
    const list = new GapList();
    for (const t of [1, 2, 3]) list.push(gap(t));
    list.dropBefore(2);
    expect(list.within(0, 10).map((g) => g.from_ms)).toEqual([2, 3]);
    list.clear();
    expect(list.length).toBe(0);
  });
});

describe("single-pass reduction matches a straightforward one", () => {
  /** The obvious per-series reduction, for comparison. */
  function reference(all: Sample[], gaps: Gap[], t0: number, t1: number, px: number): ChartFrame {
    const inWin = all.filter((s) => s.t_ms >= t0 && s.t_ms <= t1);
    const winGaps = gaps.filter((g) => g.from_ms >= t0 && g.from_ms < t1);
    const val = (s: Sample, k: string): number | null => {
      const v =
        k === "power"
          ? s.vbus_v * s.ibus_a
          : (s as unknown as Record<string, number>)[`${k}_${k === "ibus" ? "a" : "v"}`];
      return v !== undefined && Number.isFinite(v) ? v : null;
    };
    const keys = CHART_SERIES.map((s) => s.key);
    const x: number[] = [];
    const y = Object.fromEntries(keys.map((k) => [k, [] as (number | null)[]])) as ChartFrame["y"];
    const pixels = Math.max(1, Math.floor(px));
    const span = Math.max(t1 - t0, Number.EPSILON);
    const bucketOf = (t: number) => Math.min(pixels - 1, Math.floor(((t - t0) / span) * pixels));
    let g = 0;
    let i = 0;
    while (i < inWin.length) {
      const t = inWin[i]?.t_ms ?? 0;
      while (g < winGaps.length && (winGaps[g]?.from_ms ?? Infinity) < t) {
        const gp = winGaps[g] as Gap;
        const bx = (gp.from_ms + Math.min(gp.to_ms, t)) / 2;
        x.push(bx, bx);
        for (const k of keys) y[k].push(null, null);
        g++;
      }
      const nextGap = winGaps[g]?.from_ms ?? Infinity;
      const b = bucketOf(t);
      let j = i + 1;
      while (
        j < inWin.length &&
        (inWin[j]?.t_ms ?? 0) <= nextGap &&
        bucketOf(inWin[j]?.t_ms ?? 0) === b
      )
        j++;
      const bucket = inWin.slice(i, j);
      x.push(bucket[0]?.t_ms ?? 0, bucket.at(-1)?.t_ms ?? 0);
      for (const k of keys) {
        let min = Infinity;
        let max = -Infinity;
        let minAt = 0;
        let maxAt = 0;
        bucket.forEach((s, n) => {
          const v = val(s, k);
          if (v === null) return;
          if (v < min) [min, minAt] = [v, n];
          if (v > max) [max, maxAt] = [v, n];
        });
        if (min > max) y[k].push(null, null);
        else if (minAt <= maxAt) y[k].push(min, max);
        else y[k].push(max, min);
      }
      i = j;
    }
    return { x, y, decimated: true };
  }

  it("gives the same frame on a wrapped ring with gaps and bad values", () => {
    const buf = new LiveBuffer(5_000);
    const all: Sample[] = [];
    const gaps: Gap[] = [];
    let t = 0;
    for (let k = 0; k < 8_000; k++) {
      if (k % 1_700 === 1_699) {
        gaps.push({ kind: "stream", from_ms: t - 1, to_ms: t + 300 });
        buf.push(all.splice(0));
        buf.markGap(gaps.at(-1) as Gap);
        t += 300;
      }
      const s = { ...sample(t), vbus_v: Math.sin(k / 13) * 5, ibus_a: Math.cos(k / 7) };
      if (k % 997 === 0) s.dp_v = NaN;
      all.push(s);
      t += 1;
    }
    buf.push(all.splice(0));
    // rebuild the full kept sample list for the reference
    const kept: Sample[] = [];
    for (let k = 0, tt = 0; k < 8_000; k++) {
      if (k % 1_700 === 1_699) tt += 300;
      const s = { ...sample(tt), vbus_v: Math.sin(k / 13) * 5, ibus_a: Math.cos(k / 7) };
      if (k % 997 === 0) s.dp_v = NaN;
      kept.push(s);
      tt += 1;
    }
    const tail = kept.slice(-5_000);
    const first = tail[0]?.t_ms ?? 0;
    const last = tail.at(-1)?.t_ms ?? 0;
    const liveGaps = gaps.filter((g) => g.from_ms >= first);
    for (const [t0, t1, px] of [
      [first, last, 300],
      [first + 1_000, last - 500, 123],
      [first, last, 7],
    ] as const) {
      const got = buf.window(t0, t1, px);
      expect(got.decimated).toBe(true);
      expect(got).toEqual(reference(tail, liveGaps, t0, t1, px));
    }
  });
});
