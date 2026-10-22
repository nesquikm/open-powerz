/** One solid stretch of a dashed line: `[x0, y0, x1, y1]`, in the same units as the input
 * (device px when drawn). */
export type DashPiece = [number, number, number, number];

/** Lengths below this count as zero (float noise from summing sub-pixel segments). */
const EPS = 1e-9;

/** A pattern entry's length: a non-finite or negative one counts as zero. */
function dashLength(d: number | undefined): number {
  return d !== undefined && Number.isFinite(d) && d > 0 ? d : 0;
}

/** The "on" pieces of the polyline `xs`/`ys` dashed with `pattern` (on, off, on, … lengths,
 * repeated; an odd-length pattern is doubled, as the canvas does). The pattern runs on
 * across vertices and restarts after every null gap; a gap is never drawn across. An empty
 * pattern, or one with no length, draws every segment solid. No zero-length pieces.
 *
 * This lets the chart stroke dashes as a plain path, never with a canvas dash setting
 * (AC-4NSVC9.3). */
export function dashPieces(
  xs: ArrayLike<number | null | undefined>,
  ys: ArrayLike<number | null | undefined>,
  pattern: readonly number[],
): DashPiece[] {
  const out: DashPiece[] = [];
  forEachDashPiece(xs, ys, pattern, (x0, y0, x1, y1) => out.push([x0, y0, x1, y1]));
  return out;
}

/** {@link dashPieces}, handing each piece to `piece` in order instead of collecting them:
 * the chart strokes its dashes every frame without making an array per piece
 * (AC-4NSVC9.2). */
export function forEachDashPiece(
  xs: ArrayLike<number | null | undefined>,
  ys: ArrayLike<number | null | undefined>,
  pattern: readonly number[],
  piece: (x0: number, y0: number, x1: number, y1: number) => void,
): void {
  const dash = pattern.length % 2 === 1 ? [...pattern, ...pattern] : pattern;
  let total = 0;
  for (const d of dash) total += dashLength(d);
  const solid = !(total > 0);
  const entry = (k: number): number => dashLength(dash[k]);

  let k = 0;
  let left = 0;
  const restart = (): void => {
    k = 0;
    left = entry(0);
  };
  const advance = (): void => {
    // Skips zero entries; `total > 0` guarantees a non-zero one exists.
    while (left <= EPS) {
      k = (k + 1) % dash.length;
      left = entry(k);
    }
  };

  const n = Math.min(xs.length, ys.length);
  let have = false;
  let px = 0;
  let py = 0;
  for (let i = 0; i < n; i++) {
    const x = xs[i];
    const y = ys[i];
    if (x == null || y == null || !Number.isFinite(x) || !Number.isFinite(y)) {
      have = false;
      continue;
    }
    if (!have) {
      have = true;
      px = x;
      py = y;
      if (!solid) {
        restart();
        advance();
      }
      continue;
    }
    const dx = x - px;
    const dy = y - py;
    const segLen = Math.hypot(dx, dy);
    if (solid) {
      if (segLen > EPS) piece(px, py, x, y);
    } else {
      let t = 0;
      while (segLen - t > EPS) {
        const step = Math.min(left, segLen - t);
        if (k % 2 === 0 && step > EPS) {
          const a = t / segLen;
          const b = (t + step) / segLen;
          piece(px + dx * a, py + dy * a, px + dx * b, py + dy * b);
        }
        t += step;
        left -= step;
        advance();
      }
    }
    px = x;
    py = y;
  }
}
