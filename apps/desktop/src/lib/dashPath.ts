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

/** A box `{ left, top, right, bottom }` the dashes are clipped to, in the points' units. */
export interface DashClip {
  left: number;
  top: number;
  right: number;
  bottom: number;
}

/** The part `[t0, t1]` (0..1) of the segment from `(x0, y0)` by `(dx, dy)` inside `box`
 * (Liang–Barsky), written to `out`; false when none of it is inside. */
function clipRange(
  x0: number,
  y0: number,
  dx: number,
  dy: number,
  box: DashClip,
  out: [number, number],
): boolean {
  let t0 = 0;
  let t1 = 1;
  const edge = (p: number, q: number): boolean => {
    if (p === 0) return q >= 0;
    const r = q / p;
    if (p < 0) {
      if (r > t0) t0 = r;
    } else if (r < t1) t1 = r;
    return t0 <= t1;
  };
  if (
    !edge(-dx, x0 - box.left) ||
    !edge(dx, box.right - x0) ||
    !edge(-dy, y0 - box.top) ||
    !edge(dy, box.bottom - y0)
  )
    return false;
  out[0] = t0;
  out[1] = t1;
  return true;
}

/** {@link dashPieces}, handing each piece to `piece` in order instead of collecting them:
 * the chart strokes its dashes every frame without making an array per piece
 * (AC-4NSVC9.2). With `clip`, each segment is clipped to the box before it is dashed and
 * the pattern advances over the clipped-off length arithmetically, so the dashes inside
 * keep their phase and points far off the box cost nothing (AC-C6VR9K.4). It stops after
 * `cap` pieces, so no input can make one frame walk without end (AC-C6VR9K.5). Returns
 * the number of pieces handed to `piece`. */
export function forEachDashPiece(
  xs: ArrayLike<number | null | undefined>,
  ys: ArrayLike<number | null | undefined>,
  pattern: readonly number[],
  piece: (x0: number, y0: number, x1: number, y1: number) => void,
  clip?: DashClip,
  cap = Infinity,
): number {
  const dash = pattern.length % 2 === 1 ? [...pattern, ...pattern] : pattern;
  let total = 0;
  for (const d of dash) total += dashLength(d);
  const solid = !(total > 0);
  const entry = (k: number): number => dashLength(dash[k]);

  let k = 0;
  let left = 0;
  let drawn = 0;
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
  /** Moves the pattern on by `d` without walking it dash by dash. */
  const skip = (d: number): void => {
    if (d <= left) {
      left -= d;
      advance();
      return;
    }
    d = (d - left) % total;
    k = (k + 1) % dash.length;
    left = entry(k);
    // At most one pass over the pattern: `d` is now under one period.
    while (d > left) {
      d -= left;
      k = (k + 1) % dash.length;
      left = entry(k);
    }
    left -= d;
    advance();
  };
  const range: [number, number] = [0, 1];

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
    let from = 0;
    let to = segLen;
    let inside = true;
    if (clip) {
      inside = segLen > EPS && clipRange(px, py, dx, dy, clip, range);
      if (inside) {
        from = range[0] * segLen;
        to = range[1] * segLen;
      }
    }
    if (solid) {
      if (inside && to - from > EPS) {
        const a = from / segLen;
        const b = to / segLen;
        piece(px + dx * a, py + dy * a, px + dx * b, py + dy * b);
        if (++drawn >= cap) return drawn;
      }
    } else if (!inside) {
      if (segLen > EPS) skip(segLen);
    } else {
      if (from > EPS) skip(from);
      let t = from;
      while (to - t > EPS) {
        const step = Math.min(left, to - t);
        if (k % 2 === 0 && step > EPS) {
          const a = t / segLen;
          const b = (t + step) / segLen;
          piece(px + dx * a, py + dy * a, px + dx * b, py + dy * b);
          if (++drawn >= cap) return drawn;
        }
        t += step;
        left -= step;
        advance();
      }
      if (segLen - to > EPS) skip(segLen - to);
    }
    px = x;
    py = y;
  }
  return drawn;
}
