import { describe, expect, it } from "vitest";
import { dashPieces, forEachDashPiece, type DashPiece } from "./dashPath";

/** Length of one piece. */
function len([x0, y0, x1, y1]: DashPiece): number {
  return Math.hypot(x1 - x0, y1 - y0);
}

/** Total drawn ("on") length of all pieces. */
function onLength(pieces: readonly DashPiece[]): number {
  return pieces.reduce((sum, p) => sum + len(p), 0);
}

/** Compare pieces element by element, to float precision. */
function expectPieces(got: readonly DashPiece[], want: readonly DashPiece[]): void {
  expect(got.length, `piece count: ${JSON.stringify(got)}`).toBe(want.length);
  got.forEach((p, i) => {
    p.forEach((v, j) => expect(v, `piece ${i} coordinate ${j}`).toBeCloseTo(want[i]![j]!, 6));
  });
}

describe("dashPieces: dashes as solid pieces (AC-4NSVC9.3)", () => {
  it("splits a straight line into the pattern's dashes, with no zero-length pieces", () => {
    const pieces = dashPieces([0, 150], [5, 5], [10, 5]);
    expectPieces(
      pieces,
      Array.from({ length: 10 }, (_, k): DashPiece => [k * 15, 5, k * 15 + 10, 5]),
    );
    for (const p of pieces) expect(len(p)).toBeGreaterThan(0);
  });

  it("continues the pattern across vertices, so a dash spans a corner", () => {
    // 8 px right, then 8 px down: the first 10 px dash turns the corner (8 + 2),
    // the 5 px gap ends at 15, and the next dash starts 7 px down the second leg.
    const pieces = dashPieces([0, 8, 8], [0, 0, 8], [10, 5]);
    expectPieces(pieces, [
      [0, 0, 8, 0],
      [8, 0, 8, 2],
      [8, 7, 8, 8],
    ]);
  });

  it("keeps the duty cycle over a dense polyline (many sub-pixel segments)", () => {
    // 1000 points 0.3 px apart: 299.7 px of line, ~20 periods of [10, 5]. Restarting
    // the pattern at each vertex would draw it solid.
    const xs = Array.from({ length: 1000 }, (_, i) => i * 0.3);
    const ys = xs.map(() => 10);
    const total = 999 * 0.3;
    const drawn = onLength(dashPieces(xs, ys, [10, 5]));
    // Expected on-length: whole periods at 10/15, plus the on part of the remainder.
    const periods = Math.floor(total / 15);
    const expected = periods * 10 + Math.min(10, total - periods * 15);
    expect(Math.abs(drawn - expected)).toBeLessThanOrEqual(1);
    expect(drawn).toBeLessThan(total * 0.8);
  });

  it("matches a four-entry pattern's duty cycle along a diagonal", () => {
    // [10, 3, 2, 3]: 12 on of every 18. A 3-4-5 diagonal of length 180 is 10 periods.
    const xs = [0, 36, 72, 108];
    const ys = [0, 48, 96, 144];
    const drawn = onLength(dashPieces(xs, ys, [10, 3, 2, 3]));
    expect(Math.abs(drawn - 120)).toBeLessThanOrEqual(1);
  });

  it("restarts the pattern after a null gap", () => {
    // First run is 7 px (inside the first dash); the run after the gap starts a fresh
    // dash at x = 30, instead of finishing the old one's last 3 px.
    const pieces = dashPieces([0, 7, 20, 30, 37], [0, 0, null, 0, 0], [10, 5]);
    expectPieces(pieces, [
      [0, 0, 7, 0],
      [30, 0, 37, 0],
    ]);
  });

  it("never draws across a gap", () => {
    const pieces = dashPieces([0, 10, 20, 30], [0, 0, null, 0], []);
    expectPieces(pieces, [[0, 0, 10, 0]]);
  });

  it("returns nothing for an empty run, a single point or only nulls", () => {
    expect(dashPieces([], [], [10, 5])).toEqual([]);
    expect(dashPieces([3], [4], [10, 5])).toEqual([]);
    expect(dashPieces([0, 1, 2], [null, null, null], [10, 5])).toEqual([]);
    expect(dashPieces([0, 1, 2], [null, 4, null], [10, 5])).toEqual([]);
  });

  it("draws an empty or zero-length pattern solid: one piece per segment", () => {
    const xs = [0, 10, 20];
    const ys = [0, 5, 5];
    const solid: DashPiece[] = [
      [0, 0, 10, 5],
      [10, 5, 20, 5],
    ];
    expectPieces(dashPieces(xs, ys, []), solid);
    expectPieces(dashPieces(xs, ys, [0, 0]), solid);
  });
});

/**
 * FR C6VR9K: a hidden chart laid its points out ~1.3 million px off the plot, and the dash
 * builder walked those segments one dash at a time, ~1.5 million canvas calls a frame.
 *
 * API pinned here: `forEachDashPiece(xs, ys, pattern, piece, clip?, cap?)`.
 *
 * - `clip`, `{ left, top, right, bottom }` in the same units as the points: each segment
 *   is clipped to the box before it is dashed. Only pieces inside the box are handed to
 *   `piece`, and they keep the phase the unclipped pattern had (the pattern advances over
 *   the clipped-off length as if it had been walked).
 * - `cap`: at most this many pieces are handed to `piece`; the walk ends there.
 * - It returns the number of pieces handed to `piece`.
 */
describe("forEachDashPiece: clipped to the plot box (AC-C6VR9K.4)", () => {
  type Box = { left: number; top: number; right: number; bottom: number };

  /** Pieces handed out by `forEachDashPiece` with `clip`, plus its return value. */
  function clipped(
    xs: ArrayLike<number | null>,
    ys: ArrayLike<number | null>,
    pattern: readonly number[],
    clip: Box,
  ): { pieces: DashPiece[]; returned: unknown } {
    const pieces: DashPiece[] = [];
    const returned = forEachDashPiece(
      xs,
      ys,
      pattern,
      (x0, y0, x1, y1) => pieces.push([x0, y0, x1, y1]),
      clip,
    );
    return { pieces, returned };
  }

  /** `p` clipped to `box` (Liang–Barsky), or null when nothing of it is inside. */
  function clipPiece([x0, y0, x1, y1]: DashPiece, box: Box): DashPiece | null {
    const dx = x1 - x0;
    const dy = y1 - y0;
    let t0 = 0;
    let t1 = 1;
    const edges: [number, number][] = [
      [-dx, x0 - box.left],
      [dx, box.right - x0],
      [-dy, y0 - box.top],
      [dy, box.bottom - y0],
    ];
    for (const [p, q] of edges) {
      if (p === 0) {
        if (q < 0) return null;
        continue;
      }
      const r = q / p;
      if (p < 0) t0 = Math.max(t0, r);
      else t1 = Math.min(t1, r);
      if (t0 > t1) return null;
    }
    return [x0 + dx * t0, y0 + dy * t0, x0 + dx * t1, y0 + dy * t1];
  }

  /** Pieces longer than float noise: a dash touching the box edge leaves a point. */
  const real = (ps: readonly DashPiece[]): DashPiece[] => ps.filter((p) => len(p) > 1e-6);

  /** What the unclipped walk draws inside `box`. */
  function unclippedInside(
    xs: ArrayLike<number | null>,
    ys: ArrayLike<number | null>,
    pattern: readonly number[],
    box: Box,
  ): DashPiece[] {
    return real(
      dashPieces(xs, ys, pattern)
        .map((p) => clipPiece(p, box))
        .filter((p): p is DashPiece => p !== null),
    );
  }

  function expectInside(pieces: readonly DashPiece[], box: Box): void {
    for (const [x0, y0, x1, y1] of pieces) {
      for (const x of [x0, x1]) {
        expect(x).toBeGreaterThanOrEqual(box.left - 1e-6);
        expect(x).toBeLessThanOrEqual(box.right + 1e-6);
      }
      for (const y of [y0, y1]) {
        expect(y).toBeGreaterThanOrEqual(box.top - 1e-6);
        expect(y).toBeLessThanOrEqual(box.bottom + 1e-6);
      }
    }
  }

  const BOX: Box = { left: 0, top: 0, right: 1000, bottom: 500 };

  it("AC-C6VR9K.4: a 10-million-px segment draws only the dashes inside the box, in phase, fast", () => {
    // 5 000 000 px left of the box to 5 000 000 px right of it, through its middle. The
    // pattern [6, 4] starts at x = -5e6, a whole number of periods left of x = 0, so the
    // dashes inside are [10m, 10m + 6].
    const xs = [-5_000_000, 5_000_000];
    const ys = [250, 250];
    const started = performance.now();
    const { pieces, returned } = clipped(xs, ys, [6, 4], BOX);
    const took = performance.now() - started;
    const diagonal = Math.hypot(BOX.right - BOX.left, BOX.bottom - BOX.top);
    expect(pieces.length).toBeLessThanOrEqual(Math.ceil(diagonal / 10) + 2);
    expectInside(pieces, BOX);
    expectPieces(
      real(pieces),
      Array.from({ length: 100 }, (_, m): DashPiece => [10 * m, 250, 10 * m + 6, 250]),
    );
    expect(returned).toBe(pieces.length);
    expect(took, `took ${took.toFixed(1)} ms`).toBeLessThan(50);
  });

  it("AC-C6VR9K.4: a polyline far off the plot (the hidden chart's layout) draws nothing, fast", () => {
    // Every point ~1.3 million px left of the plot, as the 1 × 1 hidden chart laid them out.
    const n = 2_000;
    const xs = Array.from({ length: n }, (_, i) => -1_300_000 + i * 600);
    const ys = Array.from({ length: n }, (_, i) => (i % 2 === 0 ? -132 : 400));
    const started = performance.now();
    const { pieces, returned } = clipped(xs, ys, [6, 4], BOX);
    const took = performance.now() - started;
    expect(pieces).toEqual([]);
    expect(returned).toBe(0);
    expect(took, `took ${took.toFixed(1)} ms`).toBeLessThan(50);
  });

  it("AC-C6VR9K.4: the pattern advances over the clipped-off length, across vertices", () => {
    // 1003 px to the box's left edge, with a vertex outside the box at x = -3: at x = 0 the
    // walk is 3 px into a dash, so the box shows [0, 3], then [7, 13], [17, 23], …
    const xs = [-1003, -3, 50];
    const ys = [10, 10, 10];
    const box: Box = { left: 0, top: 0, right: 100, bottom: 20 };
    const { pieces } = clipped(xs, ys, [6, 4], box);
    expectInside(pieces, box);
    expectPieces(real(pieces), [
      [0, 10, 3, 10],
      [7, 10, 13, 10],
      [17, 10, 23, 10],
      [27, 10, 33, 10],
      [37, 10, 43, 10],
      [47, 10, 50, 10],
    ]);
  });

  it("AC-C6VR9K.4: inside the box, the clipped pieces are exactly the unclipped ones", () => {
    const box: Box = { left: 100, top: 50, right: 400, bottom: 250 };
    const cases: {
      name: string;
      xs: (number | null)[];
      ys: (number | null)[];
      pattern: number[];
    }[] = [
      {
        name: "diagonal in and out",
        xs: [-3000, 260, 2200],
        ys: [-1700, 140, 900],
        pattern: [7, 3],
      },
      {
        name: "zigzag across the edges",
        xs: [0, 150, 300, 450, 600, 120, -80],
        ys: [0, 300, -40, 280, 60, 120, 400],
        pattern: [10, 3, 2, 3],
      },
      {
        name: "a gap restarts the pattern",
        xs: [-500, 230, null, 140, 390, 900],
        ys: [120, 120, null, 60, 240, 240],
        pattern: [9, 5],
      },
      {
        name: "a segment wholly outside, then one inside",
        xs: [-900, -100, 150, 380],
        ys: [-900, -100, 100, 200],
        pattern: [5, 5],
      },
      { name: "odd pattern is doubled", xs: [-77, 520], ys: [77, 199], pattern: [4] },
    ];
    for (const c of cases) {
      const { pieces } = clipped(c.xs, c.ys, c.pattern, box);
      expectInside(pieces, box);
      const want = unclippedInside(c.xs, c.ys, c.pattern, box);
      const got = real(pieces);
      expect(got.length, `${c.name}: piece count`).toBe(want.length);
      got.forEach((p, i) =>
        p.forEach((v, j) =>
          expect(v, `${c.name}: piece ${i} coordinate ${j}`).toBeCloseTo(want[i]![j]!, 6),
        ),
      );
    }
  });

  it("AC-C6VR9K.4: a solid line is clipped to the box too", () => {
    const { pieces } = clipped([-1e7, 1e7], [250, 250], [], BOX);
    expectPieces(real(pieces), [[0, 250, 1000, 250]]);
  });

  it("AC-C6VR9K.4: no clip draws as before", () => {
    const pieces: DashPiece[] = [];
    const returned = forEachDashPiece([0, 150], [5, 5], [10, 5], (x0, y0, x1, y1) =>
      pieces.push([x0, y0, x1, y1]),
    );
    expectPieces(pieces, dashPieces([0, 150], [5, 5], [10, 5]));
    expect(pieces).toHaveLength(10);
    expect(returned).toBe(10);
  });
});

describe("forEachDashPiece: the iteration cap (AC-C6VR9K.5)", () => {
  it("AC-C6VR9K.5: stops after `cap` pieces and says how many it drew", () => {
    const all = dashPieces([0, 150], [5, 5], [10, 5]);
    expect(all).toHaveLength(10);
    const pieces: DashPiece[] = [];
    const returned = forEachDashPiece(
      [0, 150],
      [5, 5],
      [10, 5],
      (x0, y0, x1, y1) => pieces.push([x0, y0, x1, y1]),
      undefined,
      4,
    );
    expect(returned).toBe(4);
    expectPieces(pieces, all.slice(0, 4));
  });

  it("AC-C6VR9K.5: a cap ends a huge walk early, clipped or not", () => {
    // 1e7 px of [1, 1] dashes: 5 million pieces without a cap.
    let n = 0;
    const started = performance.now();
    const returned = forEachDashPiece([0, 1e7], [0, 0], [1, 1], () => n++, undefined, 1000);
    const took = performance.now() - started;
    expect(n).toBe(1000);
    expect(returned).toBe(1000);
    expect(took, `took ${took.toFixed(1)} ms`).toBeLessThan(50);

    // A zigzag inside the box: clipping alone cannot bound it, the cap does.
    const xs = Array.from({ length: 100_000 }, (_, i) => (i % 2 === 0 ? 0 : 100));
    const ys = Array.from({ length: 100_000 }, (_, i) => i * 0.0005);
    let m = 0;
    const got = forEachDashPiece(
      xs,
      ys,
      [2, 2],
      () => m++,
      { left: 0, top: 0, right: 100, bottom: 50 },
      500,
    );
    expect(m).toBe(500);
    expect(got).toBe(500);
  });
});
