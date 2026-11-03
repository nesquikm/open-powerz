import { describe, expect, it } from "vitest";
import { MIN_SPAN_MS, buildWarp, overviewClick, type TimeRange } from "./timeline";

/** FR B5XBTA: a click on the overview strip never yields a view narrower than the
 * session's narrowest allowed view, which the chart would drop. */
describe("overviewClick with a view narrower than the narrowest view (AC-B5XBTA.1)", () => {
  const WIDTH = 1000;
  /** A 100 s session whose rate allows nothing narrower than 40 ms (50 SPS). */
  const session = { firstMs: 0, lastMs: 100_000, minSpanMs: 40 };
  const flat = buildWarp([], { t0Ms: 0, t1Ms: 100_000 }, WIDTH);
  /** The live view a moment after connect: 5 ms at the session's end. */
  const narrow: TimeRange = { t0Ms: 99_995, t1Ms: 100_000 };

  it("gives exactly the narrowest view, centred on the clicked time", () => {
    expect(overviewClick(250, narrow, session, WIDTH, flat)).toEqual({
      t0Ms: 24_980,
      t1Ms: 25_020,
    });
  });

  it("keeps the narrowest view inside the session at either end", () => {
    expect(overviewClick(0, narrow, session, WIDTH, flat)).toEqual({ t0Ms: 0, t1Ms: 40 });
    expect(overviewClick(WIDTH, narrow, session, WIDTH, flat)).toEqual({
      t0Ms: 99_960,
      t1Ms: 100_000,
    });
  });

  it("an empty view (span 0) also gets the narrowest view", () => {
    const empty: TimeRange = { t0Ms: 100_000, t1Ms: 100_000 };
    expect(overviewClick(500, empty, session, WIDTH, flat)).toEqual({
      t0Ms: 49_980,
      t1Ms: 50_020,
    });
  });

  it("without a rate the floor is MIN_SPAN_MS", () => {
    const noRate = { firstMs: 0, lastMs: 100_000 };
    expect(overviewClick(500, { t0Ms: 99_998, t1Ms: 100_000 }, noRate, WIDTH, flat)).toEqual({
      t0Ms: 50_000 - MIN_SPAN_MS / 2,
      t1Ms: 50_000 + MIN_SPAN_MS / 2,
    });
  });

  it("a view exactly the narrowest keeps its width", () => {
    const exact: TimeRange = { t0Ms: 99_960, t1Ms: 100_000 };
    expect(overviewClick(250, exact, session, WIDTH, flat)).toEqual({
      t0Ms: 24_980,
      t1Ms: 25_020,
    });
  });

  it("a wider view keeps its width, as before", () => {
    const wide: TimeRange = { t0Ms: 90_000, t1Ms: 100_000 };
    expect(overviewClick(250, wide, session, WIDTH, flat)).toEqual({
      t0Ms: 20_000,
      t1Ms: 30_000,
    });
  });
});
