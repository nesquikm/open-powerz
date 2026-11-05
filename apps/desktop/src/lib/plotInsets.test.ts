/**
 * FR AXHK2T (AC-AXHK2T.6): the sub-chart shares the main plot's time range, so the same
 * ms must land at the same x in both. Each plot sizes its value axes from its own tick
 * labels and shows its own series, so `SharedInsets` pads each plot's left and right to
 * the wider of the two, after layout: both plot areas then start and end at the same x.
 */

import { describe, expect, it, vi } from "vitest";
import type uPlot from "uplot";
import { SharedInsets } from "./plotInsets";

type SideFn = () => number;

/** A plot `width` px wide whose axes take `left` and `right` px on their own; laying it
 * out applies the padding the way uPlot does. */
function fakePlot(width: number, left: number, right: number) {
  const over = { style: { left: "", width: "" } };
  const u = { width, over } as unknown as uPlot;
  let padding: uPlot.Padding = [null, null, null, null];
  const layout = (): void => {
    const l = left + ((padding[3] as SideFn | null)?.() ?? 0);
    const r = right + ((padding[1] as SideFn | null)?.() ?? 0);
    over.style.left = `${l}px`;
    over.style.width = `${width - l - r}px`;
  };
  return {
    u,
    layout,
    use(p: uPlot.Padding) {
      padding = p;
      layout();
    },
    setAxes(l: number, r: number) {
      left = l;
      right = r;
      layout();
    },
    area: () => [parseFloat(over.style.left), parseFloat(over.style.width)],
  };
}

function attach(insets: SharedInsets, id: string, p: ReturnType<typeof fakePlot>) {
  const relayout = vi.fn(() => p.layout());
  const a = insets.attach(id, relayout);
  p.use(a.padding);
  a.bind(p.u);
  return { ...a, relayout };
}

describe("SharedInsets (AC-AXHK2T.6)", () => {
  it("pads each plot to the wider of the two on each side", () => {
    const insets = new SharedInsets();
    const main = fakePlot(1000, 60, 90);
    const sub = fakePlot(1000, 45, 25);
    attach(insets, "main", main);
    attach(insets, "sub", sub);
    insets.align();
    expect(main.area()).toEqual([60, 850]);
    expect(sub.area()).toEqual(main.area());
  });

  it("does nothing more once aligned", () => {
    const insets = new SharedInsets();
    const main = attach(insets, "main", fakePlot(1000, 60, 90));
    const sub = attach(insets, "sub", fakePlot(1000, 45, 25));
    insets.align();
    main.relayout.mockClear();
    sub.relayout.mockClear();
    insets.align();
    expect(main.relayout).not.toHaveBeenCalled();
    expect(sub.relayout).not.toHaveBeenCalled();
  });

  it("follows an axis that grows or shrinks, on either plot", () => {
    const insets = new SharedInsets();
    const main = fakePlot(1000, 60, 90);
    const sub = fakePlot(1000, 45, 25);
    attach(insets, "main", main);
    attach(insets, "sub", sub);
    insets.align();
    sub.setAxes(80, 25);
    insets.align();
    expect(main.area()).toEqual([80, 830]);
    expect(sub.area()).toEqual(main.area());
    sub.setAxes(30, 25);
    insets.align();
    expect(main.area()).toEqual([60, 850]);
    expect(sub.area()).toEqual(main.area());
  });

  it("a detached plot no longer widens the others", () => {
    const insets = new SharedInsets();
    const main = fakePlot(1000, 40, 25);
    attach(insets, "main", main);
    const sub = attach(insets, "sub", fakePlot(1000, 80, 25));
    insets.align();
    expect(main.area()[0]).toBe(80);
    sub.detach();
    expect(main.area()[0]).toBe(40);
  });

  it("a plot not laid out yet is left alone", () => {
    const insets = new SharedInsets();
    const main = attach(insets, "main", fakePlot(1000, 60, 90));
    insets.attach("sub", vi.fn());
    insets.align();
    expect(main.relayout).not.toHaveBeenCalled();
  });
});
