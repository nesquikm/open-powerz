// FR 9QTPNP: while the live window fills (buffer shorter than the span), smoothing lets the
// window's end trail the newest sample by up to one batch. The window's start must stay on the
// first sample and not move back by that lag; only the end moves.
//
// Contract under test (chartData.ts):
//
//   smoothLiveRange(liveEndMs, spanMs, firstMs) -> [startMs, endMs]
//     liveEndMs  the smoothed window end (smoothLiveEnd(...).endMs)
//     spanMs     the window span
//     firstMs    the live buffer's first sample time
//     returns    [max(firstMs, liveEndMs - spanMs), liveEndMs]
import { describe, expect, it } from "vitest";
import { LIVE_BATCH_INTERVAL_MS, smoothLiveRange } from "./chartData";

const SPAN_MS = 10_000;

describe("smoothLiveRange", () => {
  describe("AC-9QTPNP.1: window still filling", () => {
    it("starts at the first sample and ends at the smoothed end", () => {
      // buffer covers 1000..3000 ms; the smoothed end trails the newest sample by 38 ms
      expect(smoothLiveRange(2962, SPAN_MS, 1000)).toEqual([1000, 2962]);
    });

    it("keeps the start on the first sample on every frame while the end advances", () => {
      const firstMs = 5000;
      const newestMs = 7000;
      const starts: number[] = [];
      const ends: number[] = [];
      // the smoothing lag goes from a full batch down to zero over the frames
      for (let lag = LIVE_BATCH_INTERVAL_MS; lag >= 0; lag -= 3) {
        const liveEnd = newestMs - lag;
        const [start, end] = smoothLiveRange(liveEnd, SPAN_MS, firstMs);
        starts.push(start);
        ends.push(end);
        expect(end).toBe(liveEnd);
      }
      expect(new Set(starts)).toEqual(new Set([firstMs]));
      for (let i = 1; i < ends.length; i++) expect(ends[i]).toBeGreaterThan(ends[i - 1]!);
    });

    it("never starts before the first sample right after the stream starts", () => {
      // first frame: one batch in, the smoothed end is before even the first sample + span
      const [start, end] = smoothLiveRange(0 + 12, SPAN_MS, 0);
      expect(start).toBe(0);
      expect(end).toBe(12);
    });
  });

  describe("AC-9QTPNP.2: window full", () => {
    it("is the full span ending at the smoothed end", () => {
      // buffer covers 0..30000 ms, smoothed end lags 47 ms
      expect(smoothLiveRange(29_953, SPAN_MS, 0)).toEqual([19_953, 29_953]);
    });

    it("slides start and end together once full", () => {
      const a = smoothLiveRange(20_000, SPAN_MS, 1000);
      const b = smoothLiveRange(20_050, SPAN_MS, 1000);
      expect(a).toEqual([10_000, 20_000]);
      expect(b).toEqual([10_050, 20_050]);
    });

    it("at the transition the start is exactly the first sample", () => {
      expect(smoothLiveRange(11_000, SPAN_MS, 1000)).toEqual([1000, 11_000]);
      // one ms short of full: clamped to the first sample, not 999
      expect(smoothLiveRange(10_999, SPAN_MS, 1000)).toEqual([1000, 10_999]);
      // one ms past full: the full span
      expect(smoothLiveRange(11_001, SPAN_MS, 1000)).toEqual([1001, 11_001]);
    });
  });
});
