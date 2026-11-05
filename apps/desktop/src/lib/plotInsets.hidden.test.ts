/**
 * FR C6VR9K, AC .3: the Meter tab's chart stays mounted while another tab shows, and
 * uPlot sizes it 1 × 1 there. `SharedInsets` used to pad that plot to the sub-chart's
 * insets anyway, which turned its plot area negative (height −132) and put its points a
 * million px off the plot; the insets it learned while hidden also misplaced the PD
 * markers after the return.
 *
 * Pinned here: a member whose plot is not laid out (its plot area has no positive width
 * or height: hidden at 1 × 1, or smaller than its own axes) takes no part in `align()`:
 * it sets no target, gets no extra padding and is not laid out again; the others align as
 * if it were not there. Laid out again at its real size, it aligns with them once more.
 */

import { describe, expect, it, vi } from "vitest";
import type uPlot from "uplot";
import { SharedInsets } from "./plotInsets";

type SideFn = () => number;

/** Axis space above and below the plot area, CSS px. */
const TOP = 10;
const BOTTOM = 30;

/** A plot `width` × `height` CSS px whose axes take `left` and `right` px on their own;
 * laying it out applies the padding the way uPlot does, `over` and `bbox` alike. */
function fakePlot(width: number, height: number, left: number, right: number) {
  const over = { style: { left: "", top: "", width: "", height: "" } };
  const u = { width, height, over, bbox: { left: 0, top: 0, width: 0, height: 0 } };
  let padding: uPlot.Padding = [null, null, null, null];
  let extra: [number, number] = [0, 0];
  const layout = (): void => {
    extra = [(padding[3] as SideFn | null)?.() ?? 0, (padding[1] as SideFn | null)?.() ?? 0];
    const l = left + extra[0];
    const r = right + extra[1];
    over.style.left = `${l}px`;
    over.style.top = `${TOP}px`;
    over.style.width = `${u.width - l - r}px`;
    over.style.height = `${u.height - TOP - BOTTOM}px`;
    u.bbox = { left: l, top: TOP, width: u.width - l - r, height: u.height - TOP - BOTTOM };
  };
  return {
    u: u as unknown as uPlot,
    layout,
    use(p: uPlot.Padding) {
      padding = p;
      layout();
    },
    resize(w: number, h: number) {
      u.width = w;
      u.height = h;
      layout();
    },
    /** The plot area's left inset and width, CSS px. */
    area: () => [parseFloat(over.style.left), parseFloat(over.style.width)],
    /** The padding uPlot would apply now, left and right. */
    padding: (): [number, number] => [
      (padding[3] as SideFn | null)?.() ?? 0,
      (padding[1] as SideFn | null)?.() ?? 0,
    ],
  };
}

function attach(insets: SharedInsets, id: string, p: ReturnType<typeof fakePlot>) {
  const relayout = vi.fn(() => p.layout());
  const a = insets.attach(id, relayout);
  p.use(a.padding);
  a.bind(p.u);
  return { ...a, relayout };
}

describe("SharedInsets skips a plot that is not laid out (AC-C6VR9K.3)", () => {
  it("AC-C6VR9K.3: a hidden 1 × 1 plot is not padded and does not widen the others", () => {
    const insets = new SharedInsets();
    const sub = fakePlot(1000, 200, 45, 25);
    const hidden = fakePlot(1, 1, 60, 90);
    attach(insets, "sub", sub);
    const h = attach(insets, "main", hidden);
    insets.align();
    expect(hidden.padding()).toEqual([0, 0]);
    expect(h.relayout).not.toHaveBeenCalled();
    // The sub-chart keeps its own insets: nothing hidden widens it.
    expect(sub.padding()).toEqual([0, 0]);
    expect(sub.area()).toEqual([45, 930]);
  });

  it("AC-C6VR9K.3: a plot smaller than its axes counts as not laid out", () => {
    for (const [w, hgt] of [
      [100, 300], // 100 px wide, 150 px of axes: plot width −50
      [1000, 20], // 20 px tall, 40 px of axes: plot height −20
      [150, 300], // exactly its axes: no width
    ] as const) {
      const insets = new SharedInsets();
      const sub = fakePlot(1000, 200, 45, 25);
      const small = fakePlot(w, hgt, 60, 90);
      attach(insets, "sub", sub);
      const s = attach(insets, "main", small);
      insets.align();
      expect(small.padding(), `${w} × ${hgt}`).toEqual([0, 0]);
      expect(s.relayout, `${w} × ${hgt}`).not.toHaveBeenCalled();
      expect(sub.area(), `${w} × ${hgt}`).toEqual([45, 930]);
    }
  });

  it("AC-C6VR9K.3: hiding a plot drops it from the alignment; the others align without it", () => {
    const insets = new SharedInsets();
    const main = fakePlot(1000, 300, 60, 90);
    const sub = fakePlot(1000, 200, 45, 25);
    attach(insets, "main", main);
    attach(insets, "sub", sub);
    insets.align();
    expect(sub.area()).toEqual(main.area());
    // The tab changes: uPlot sizes the main plot 1 × 1.
    main.resize(1, 1);
    insets.align();
    // The sub-chart (still shown, say) returns to its own insets …
    expect(sub.area()).toEqual([45, 930]);
    // … and the hidden plot's padding never pushes its plot area further negative.
    expect(main.padding()).toEqual([0, 0]);
    for (let i = 0; i < 5; i++) insets.align();
    expect(main.padding()).toEqual([0, 0]);
    expect(sub.area()).toEqual([45, 930]);
  });

  it("AC-C6VR9K.3: shown again at its real size, it aligns with the sub-chart once more", () => {
    const insets = new SharedInsets();
    const main = fakePlot(1000, 300, 60, 90);
    const sub = fakePlot(1000, 200, 45, 25);
    attach(insets, "main", main);
    const s = attach(insets, "sub", sub);
    insets.align();
    main.resize(1, 1);
    insets.align();
    main.resize(1000, 300);
    insets.align();
    expect(main.area()).toEqual([60, 850]);
    expect(sub.area()).toEqual(main.area());
    // Settled: another align changes nothing.
    s.relayout.mockClear();
    insets.align();
    expect(s.relayout).not.toHaveBeenCalled();
  });

  it("AC-C6VR9K.3: two hidden plots leave each other alone", () => {
    const insets = new SharedInsets();
    const a = fakePlot(1, 1, 60, 90);
    const b = fakePlot(1, 1, 45, 25);
    const ma = attach(insets, "main", a);
    const mb = attach(insets, "sub", b);
    insets.align();
    expect(a.padding()).toEqual([0, 0]);
    expect(b.padding()).toEqual([0, 0]);
    expect(ma.relayout).not.toHaveBeenCalled();
    expect(mb.relayout).not.toHaveBeenCalled();
  });
});
