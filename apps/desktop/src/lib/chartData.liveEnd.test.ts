// FR HR2JAG: the backend sends live batches at 20 Hz, so the smooth live chart moves its
// window's end on with the wall clock between batches instead of jumping once per batch —
// but only where one batch interval moves the plot by at least 2 px (the short windows).
// Below that the end sits on the newest sample as before.
//
// Contract under test (chartData.ts):
//
//   LIVE_BATCH_INTERVAL_MS = 50      // the backend's flush interval (20 Hz)
//
//   smoothLiveEnd(prev, newestMs, arrivedAtMs, nowMs, lagMs = LIVE_BATCH_INTERVAL_MS)
//     -> { endMs }
//     prev        the previous frame's result, or null on the first frame of a stream
//     newestMs    the newest sample's device time
//     arrivedAtMs wall time (ms) the batch carrying that sample arrived
//     nowMs       wall time (ms) of the frame being drawn
//     endMs       newestMs - lagMs + (nowMs - arrivedAtMs), clamped to at most newestMs,
//                 at least newestMs - lagMs, and never below prev.endMs
//
//   smoothingActive(spanMs, plotWidthPx, intervalMs = LIVE_BATCH_INTERVAL_MS) -> boolean
//     true iff intervalMs * plotWidthPx / spanMs >= 2 (one batch moves the plot >= 2 px);
//     false for non-finite, zero or negative inputs
//
//   liveNeedsFrame({ live, visible, smooth, endMs, newestMs }) -> boolean
//     true when the chart must redraw on the next animation frame although no batch came;
//     `smooth` means "smoothing active": motion is smooth AND smoothingActive(...)
//
//   LIVE_SMOOTH_MIN_FRAME_MS = 1000 / 30   // smoothing redraws at most 30 times a second
//   (the chart's RedrawScheduler applies it to every redraw while smoothing; see chartView.test.ts)
import { describe, expect, it } from "vitest";
import {
  LIVE_BATCH_INTERVAL_MS,
  LIVE_SMOOTH_MIN_FRAME_MS,
  liveNeedsFrame,
  smoothingActive,
  smoothLiveEnd,
  steppedRange,
  type LiveEnd,
} from "./chartData";

const FRAME_MS = 1000 / 60;

describe("LIVE_BATCH_INTERVAL_MS (AC-HR2JAG.2)", () => {
  it("is the backend's 20 Hz flush interval", () => {
    expect(LIVE_BATCH_INTERVAL_MS).toBe(50);
  });
});

describe("smoothLiveEnd (AC-HR2JAG.2)", () => {
  it("starts one batch interval behind the newest sample", () => {
    expect(smoothLiveEnd(null, 10_000, 500, 500).endMs).toBe(10_000 - LIVE_BATCH_INTERVAL_MS);
  });

  it("moves on with the wall clock between batches", () => {
    const first = smoothLiveEnd(null, 10_000, 500, 500);
    const a = smoothLiveEnd(first, 10_000, 500, 520);
    expect(a.endMs).toBeCloseTo(9_970, 6);
    const b = smoothLiveEnd(a, 10_000, 500, 540);
    expect(b.endMs).toBeCloseTo(9_990, 6);
  });

  it("never passes the newest sample when the next batch is late", () => {
    let s = smoothLiveEnd(null, 10_000, 500, 500);
    s = smoothLiveEnd(s, 10_000, 500, 550);
    expect(s.endMs).toBe(10_000);
    s = smoothLiveEnd(s, 10_000, 500, 700);
    expect(s.endMs).toBe(10_000);
    s = smoothLiveEnd(s, 10_000, 500, 5_000);
    expect(s.endMs).toBe(10_000);
  });

  it("scrolls without a jump back when batches come on time", () => {
    // Ten on-time batches: each carries 50 ms of samples and arrives 50 ms after the last;
    // frames run at 60 Hz on the wall clock.
    let prev: LiveEnd | null = null;
    const all: number[] = [];
    for (let i = 0; ; i++) {
      const now = 500 + i * FRAME_MS;
      if (now >= 1_000) break;
      const batch = Math.floor((now - 500) / 50);
      const newest = 10_000 + batch * 50;
      const arrived = 500 + batch * 50;
      prev = smoothLiveEnd(prev, newest, arrived, now);
      expect(prev.endMs).toBeLessThanOrEqual(newest);
      expect(newest - prev.endMs).toBeLessThanOrEqual(LIVE_BATCH_INTERVAL_MS);
      all.push(prev.endMs);
    }
    // It really moves between batches, and never back: every frame shows a later end.
    for (let i = 1; i < all.length; i++) expect(all[i]).toBeGreaterThan(all[i - 1]!);
  });

  it("catches up after a late batch to trail by at most one interval", () => {
    let s = smoothLiveEnd(null, 10_000, 500, 500);
    s = smoothLiveEnd(s, 10_000, 500, 600); // waiting: clamped at the newest sample
    expect(s.endMs).toBe(10_000);
    // The late batch brings 150 ms of samples at once.
    s = smoothLiveEnd(s, 10_150, 620, 620);
    expect(s.endMs).toBe(10_150 - LIVE_BATCH_INTERVAL_MS);
    s = smoothLiveEnd(s, 10_150, 620, 650);
    expect(s.endMs).toBeCloseTo(10_130, 6);
    expect(s.endMs).toBeLessThanOrEqual(10_150);
  });

  it("never moves back when a batch comes early", () => {
    let s = smoothLiveEnd(null, 10_000, 500, 500);
    s = smoothLiveEnd(s, 10_000, 500, 545);
    expect(s.endMs).toBeCloseTo(9_995, 6);
    // An early, small batch: its newest sample minus the lag is behind the shown end.
    s = smoothLiveEnd(s, 10_020, 545, 545);
    expect(s.endMs).toBeCloseTo(9_995, 6);
    // ...holds until the wall clock catches up, then moves on to the new newest sample.
    s = smoothLiveEnd(s, 10_020, 545, 560);
    expect(s.endMs).toBeCloseTo(9_995, 6);
    s = smoothLiveEnd(s, 10_020, 545, 580);
    expect(s.endMs).toBeCloseTo(10_005, 6);
    expect(s.endMs).toBeLessThanOrEqual(10_020);
    s = smoothLiveEnd(s, 10_020, 545, 700);
    expect(s.endMs).toBe(10_020);
  });

  it("ignores a frame time before the batch's arrival", () => {
    const s = smoothLiveEnd(null, 10_000, 500, 480);
    expect(s.endMs).toBe(10_000 - LIVE_BATCH_INTERVAL_MS);
  });

  it("honours a custom lag", () => {
    expect(smoothLiveEnd(null, 10_000, 500, 500, 100).endMs).toBe(9_900);
    expect(smoothLiveEnd(null, 10_000, 500, 560, 100).endMs).toBe(9_960);
  });
});

describe("smoothingActive: the 2 px switch (AC-HR2JAG.2)", () => {
  it("is active for a 10 s window at 1200 px (6 px per batch)", () => {
    expect(smoothingActive(10_000, 1_200)).toBe(true);
  });

  it("is inactive for a 1 min window at 1200 px (1 px per batch)", () => {
    expect(smoothingActive(60_000, 1_200)).toBe(false);
  });

  it("is active at exactly 2 px per batch (1 min at 2400 px)", () => {
    expect(smoothingActive(60_000, 2_400)).toBe(true);
  });

  it("is inactive just under 2 px per batch", () => {
    expect(smoothingActive(60_000, 2_399)).toBe(false);
  });

  it("is inactive for the long windows (10 min) at any common width", () => {
    expect(smoothingActive(600_000, 3_840)).toBe(false);
  });

  it("honours a custom batch interval", () => {
    // 100 ms batches on a 1 min window at 1200 px move the plot 2 px.
    expect(smoothingActive(60_000, 1_200, 100)).toBe(true);
  });

  it("is inactive for zero, negative or non-finite inputs", () => {
    expect(smoothingActive(0, 1_200)).toBe(false);
    expect(smoothingActive(10_000, 0)).toBe(false);
    expect(smoothingActive(-10_000, 1_200)).toBe(false);
    expect(smoothingActive(10_000, -1_200)).toBe(false);
    expect(smoothingActive(Number.NaN, 1_200)).toBe(false);
    expect(smoothingActive(10_000, Number.NaN)).toBe(false);
    expect(smoothingActive(Number.POSITIVE_INFINITY, 1_200)).toBe(false);
    expect(smoothingActive(10_000, Number.POSITIVE_INFINITY)).toBe(false);
    expect(smoothingActive(10_000, 1_200, 0)).toBe(false);
  });

  it("an inactive switch never asks for frames between batches", () => {
    const smooth = smoothingActive(60_000, 1_200); // 1 px per batch
    expect(
      liveNeedsFrame({ live: true, visible: true, smooth, endMs: 9_970, newestMs: 10_000 }),
    ).toBe(false);
  });
});

describe("the smoothing frame cap (AC-HR2JAG.3)", () => {
  it("caps smoothing at 30 frames a second", () => {
    expect(LIVE_SMOOTH_MIN_FRAME_MS).toBeCloseTo(1000 / 30, 9);
  });
});

describe("liveNeedsFrame (AC-HR2JAG.3)", () => {
  const base = { live: true, visible: true, smooth: true, endMs: 9_970, newestMs: 10_000 };

  it("asks for frames while smoothing is active, live, visible and behind the newest sample", () => {
    expect(liveNeedsFrame(base)).toBe(true);
  });

  it("stops once the end has caught up with the newest sample", () => {
    expect(liveNeedsFrame({ ...base, endMs: 10_000 })).toBe(false);
  });

  it("does not redraw a hidden chart", () => {
    expect(liveNeedsFrame({ ...base, visible: false })).toBe(false);
  });

  it("does not redraw a paused (history) chart", () => {
    expect(liveNeedsFrame({ ...base, live: false })).toBe(false);
  });

  it("does not animate under reduced motion or when smoothing is inactive", () => {
    expect(liveNeedsFrame({ ...base, smooth: false })).toBe(false);
  });

  it("reduced motion keeps its tenth-of-a-span steps for a smoothed end", () => {
    // A smoothed end between steps still lands on a tenth-of-a-span boundary.
    const end = smoothLiveEnd(null, 20_730, 500, 520).endMs; // 20_700
    expect(steppedRange([end - 10_000, end], 10_000, 0)).toEqual([10_000, 20_000]);
  });
});
