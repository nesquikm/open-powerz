import { describe, expect, it } from "vitest";
import { dashPieces, type DashPiece } from "./dashPath";

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
