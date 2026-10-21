import { describe, expect, it } from "vitest";
import { bridgeColumns } from "./stripEnvelope";

/** Per-column envelopes; `null` is an empty column (lo = +Inf, hi = -Inf). */
function columns(spec: readonly ([number, number] | null)[]): {
  lo: Float64Array;
  hi: Float64Array;
} {
  const lo = new Float64Array(spec.length).fill(Infinity);
  const hi = new Float64Array(spec.length).fill(-Infinity);
  spec.forEach((c, i) => {
    if (c === null) return;
    lo[i] = c[0];
    hi[i] = c[1];
  });
  return { lo, hi };
}

function filled(lo: Float64Array, hi: Float64Array, c: number): boolean {
  return hi[c]! >= lo[c]!;
}

/** Consecutive filled columns whose drawn ranges touch or overlap (a continuous line). */
function touches(lo: Float64Array, hi: Float64Array, a: number, b: number): boolean {
  return lo[b]! <= hi[a]! && lo[a]! <= hi[b]!;
}

const never = (): boolean => false;

describe("bridgeColumns: the strip draws sparse series as a line (AC-SX3EDV.6)", () => {
  it("fills empty columns between two samples so every column joins its neighbour", () => {
    const { lo, hi } = columns([[1, 1], null, null, null, [5, 5]]);
    bridgeColumns(lo, hi, never);
    for (let c = 0; c < 5; c++) expect(filled(lo, hi, c), `column ${c} drawn`).toBe(true);
    for (let c = 1; c < 5; c++) {
      expect(touches(lo, hi, c - 1, c), `columns ${c - 1} and ${c} touch`).toBe(true);
    }
    // The bridge follows the line between the two samples, not a block over the span.
    expect(lo[2]!).toBeGreaterThan(1);
    expect(hi[2]!).toBeLessThan(5);
  });

  it("keeps a flat series flat across the empty columns", () => {
    const { lo, hi } = columns([[5, 5], null, null, [5, 5]]);
    bridgeColumns(lo, hi, never);
    for (let c = 0; c < 4; c++) {
      expect(lo[c]).toBe(5);
      expect(hi[c]).toBe(5);
    }
  });

  it("extends adjacent non-empty columns with disjoint ranges until they touch", () => {
    const { lo, hi } = columns([
      [1, 1],
      [3, 3],
      [2, 2.5],
    ]);
    bridgeColumns(lo, hi, never);
    expect(touches(lo, hi, 0, 1)).toBe(true);
    expect(touches(lo, hi, 1, 2)).toBe(true);
    // Each column still covers its own samples.
    expect(lo[0]!).toBeLessThanOrEqual(1);
    expect(hi[1]!).toBeGreaterThanOrEqual(3);
    expect(lo[2]!).toBeLessThanOrEqual(2);
    expect(hi[2]!).toBeGreaterThanOrEqual(2.5);
  });

  it("leaves columns that already touch as they are", () => {
    const { lo, hi } = columns([
      [1, 2],
      [2, 3],
      [2.5, 2.8],
    ]);
    bridgeColumns(lo, hi, never);
    expect([...lo]).toEqual([1, 2, 2.5]);
    expect([...hi]).toEqual([2, 3, 2.8]);
  });

  it("leaves leading and trailing empty columns empty", () => {
    const { lo, hi } = columns([null, null, [1, 1], null, [2, 2], null, null]);
    bridgeColumns(lo, hi, never);
    for (const c of [0, 1, 5, 6]) expect(filled(lo, hi, c), `column ${c} empty`).toBe(false);
    expect(filled(lo, hi, 3)).toBe(true);
  });

  it("never fills a column a gap or break covers, nor bridges across one", () => {
    const { lo, hi } = columns([[1, 1], null, null, null, null, null, [5, 5]]);
    const blocked = (c: number): boolean => c === 3 || c === 4;
    bridgeColumns(lo, hi, blocked);
    expect(filled(lo, hi, 3)).toBe(false);
    expect(filled(lo, hi, 4)).toBe(false);
    // Nothing across the gap: the open columns either side stay empty too.
    for (const c of [1, 2, 5]) expect(filled(lo, hi, c), `column ${c} not bridged`).toBe(false);
    expect([lo[0], hi[0], lo[6], hi[6]]).toEqual([1, 1, 5, 5]);
  });

  it("does not join two samples on either side of a gap that sit in adjacent columns", () => {
    const { lo, hi } = columns([[1, 1], null, [4, 4]]);
    bridgeColumns(lo, hi, (c) => c === 1);
    expect(filled(lo, hi, 1)).toBe(false);
    expect([lo[0], hi[0], lo[2], hi[2]]).toEqual([1, 1, 4, 4]);
  });

  it("does not extend a sample column into its neighbour when a blocked column is between", () => {
    const { lo, hi } = columns([
      [1, 1],
      [9, 9],
      [3, 3],
    ]);
    // Column 1 itself is blocked: a sample drawn on a break edge is not joined across it.
    bridgeColumns(lo, hi, (c) => c === 1);
    expect([lo[0], hi[0]]).toEqual([1, 1]);
    expect([lo[2], hi[2]]).toEqual([3, 3]);
  });

  it("never writes NaN when a column holds a non-finite sample", () => {
    const { lo, hi } = columns([[1, 1], null, [Infinity, Infinity], null, [3, 3]]);
    bridgeColumns(lo, hi, () => false);
    for (let c = 0; c < lo.length; c++) {
      expect(Number.isNaN(lo[c]!), `lo[${c}]`).toBe(false);
      expect(Number.isNaN(hi[c]!), `hi[${c}]`).toBe(false);
    }
  });

  it("leaves a single sample, an empty strip and an all-blocked strip as they are", () => {
    const one = columns([null, [2, 2], null]);
    bridgeColumns(one.lo, one.hi, () => false);
    expect([filled(one.lo, one.hi, 0), filled(one.lo, one.hi, 2)]).toEqual([false, false]);
    expect([one.lo[1], one.hi[1]]).toEqual([2, 2]);

    const none = columns([null, null]);
    bridgeColumns(none.lo, none.hi, () => false);
    expect([filled(none.lo, none.hi, 0), filled(none.lo, none.hi, 1)]).toEqual([false, false]);

    const all = columns([[1, 1], null, [5, 5]]);
    bridgeColumns(all.lo, all.hi, () => true);
    expect([all.lo[0], all.hi[0], all.lo[2], all.hi[2]]).toEqual([1, 1, 5, 5]);
    expect(filled(all.lo, all.hi, 1)).toBe(false);
  });
});
