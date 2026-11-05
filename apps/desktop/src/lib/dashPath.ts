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

/** Anchors a dash pattern in data time: `ts[i]` is point i's data time, and the pattern
 * restarts at every multiple of `stepMs` the line crosses (FR SX1VTC). */
export interface DashAnchor {
  ts: ArrayLike<number | null | undefined>;
  stepMs: number;
}

/** The grid step (ms) a dash pattern of `patternPeriodPx` restarts on at `msPerPx`: the
 * smallest 1-2-5 × 10^k ms whose cell spans at least four pattern lengths on screen
 * (AC-SX1VTC.2). Bad input gives a finite, positive step. */
export function dashGridStepMs(msPerPx: number, patternPeriodPx: number): number {
  const need = msPerPx * 4 * patternPeriodPx;
  if (!Number.isFinite(need) || !(need > 0)) return 1;
  let e = Math.floor(Math.log10(need));
  for (;;) {
    for (const m of [1, 2, 5]) {
      const s = e < 0 ? m / 10 ** -e : m * 10 ** e;
      if (s >= need * (1 - 1e-12)) return s;
    }
    e += 1;
  }
}

/** The grid step (ms) `pattern` (device px) restarts on in a view of `xMin..xMax` laid
 * over `widthPx` device px, or 0 when it cannot be anchored (a solid pattern, or no
 * finite view): the dashes then run from the first point (FR SX1VTC). */
export function dashAnchorStepMs(
  pattern: readonly number[],
  xMin: number | null | undefined,
  xMax: number | null | undefined,
  widthPx: number,
): number {
  let period = 0;
  for (const d of pattern) if (Number.isFinite(d) && d > 0) period += d;
  if (pattern.length % 2 === 1) period *= 2;
  if (!(period > 0) || xMin == null || xMax == null || !Number.isFinite(xMin)) return 0;
  return dashGridStepMs((xMax - xMin) / widthPx, period);
}

/** {@link dashPieces}, handing each piece to `piece` in order instead of collecting them:
 * the chart strokes its dashes every frame without making an array per piece
 * (AC-4NSVC9.2). With `clip`, each segment is clipped to the box before it is dashed and
 * the pattern advances over the clipped-off length arithmetically, so the dashes inside
 * keep their phase and points far off the box cost nothing (AC-C6VR9K.4). It stops after
 * `cap` pieces, so no input can make one frame walk without end (AC-C6VR9K.5). With
 * `anchor`, the pattern restarts at phase 0 wherever the line crosses a multiple of
 * `anchor.stepMs` in data time (at the point interpolated on the segment), so a scrolled
 * view keeps every dash on the same samples (AC-SX1VTC.1). Returns the number of pieces
 * handed to `piece`. */
export function forEachDashPiece(
  xs: ArrayLike<number | null | undefined>,
  ys: ArrayLike<number | null | undefined>,
  pattern: readonly number[],
  piece: (x0: number, y0: number, x1: number, y1: number) => void,
  clip?: DashClip,
  cap = Infinity,
  anchor?: DashAnchor,
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
    if (!solid) advance();
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

  /** Dashes the segment `(ax, ay)` → `(bx, by)`; true once `cap` pieces are drawn. */
  const segment = (ax: number, ay: number, bx: number, by: number): boolean => {
    const dx = bx - ax;
    const dy = by - ay;
    const segLen = Math.hypot(dx, dy);
    let from = 0;
    let to = segLen;
    let inside = true;
    if (clip) {
      inside = segLen > EPS && clipRange(ax, ay, dx, dy, clip, range);
      if (inside) {
        from = range[0] * segLen;
        to = range[1] * segLen;
      }
    }
    if (solid) {
      if (inside && to - from > EPS) {
        const a = from / segLen;
        const b = to / segLen;
        piece(ax + dx * a, ay + dy * a, ax + dx * b, ay + dy * b);
        if (++drawn >= cap) return true;
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
          piece(ax + dx * a, ay + dy * a, ax + dx * b, ay + dy * b);
          if (++drawn >= cap) return true;
        }
        t += step;
        left -= step;
        advance();
      }
      if (segLen - to > EPS) skip(segLen - to);
    }
    return false;
  };
  const stepMs = anchor?.stepMs ?? 0;
  const stepOk = Number.isFinite(stepMs) && stepMs > 0;
  let pt: number | null = null;
  let splits = 0;
  const wholeRange: [number, number] = [0, 1];

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
      if (!solid) restart();
      const t0 = anchor && stepOk ? anchor.ts[i] : null;
      pt = t0 != null && Number.isFinite(t0) ? t0 : null;
      continue;
    }
    const t = anchor && stepOk ? anchor.ts[i] : null;
    if (t != null && pt != null && Number.isFinite(t) && t > pt) {
      // Split the segment at every grid time it crosses, restarting the pattern there.
      let g = Math.floor(pt / stepMs) + 1;
      if (g * stepMs <= pt) g += 1;
      let gEnd = Math.floor(t / stepMs);
      if (gEnd * stepMs > t) gEnd -= 1;
      // Only the grid times on the clipped part are walked one by one, plus the last one
      // before it and the last one of all: the others fall off the box, where only the
      // latest restart counts. So a segment long in time costs no more than the cells it
      // shows, and every split counts toward `cap` (AC-SX1VTC.5).
      let gA = g;
      let gB = gEnd;
      if (clip && g <= gEnd) {
        const span = t - pt;
        if (clipRange(px, py, x - px, y - py, clip, wholeRange)) {
          gA = Math.max(g, Math.floor((pt + wholeRange[0] * span) / stepMs) - 1);
          gB = Math.min(gEnd, Math.ceil((pt + wholeRange[1] * span) / stepMs) + 1);
        } else {
          gA = gEnd;
          gB = gEnd;
        }
      }
      let sx = px;
      let sy = py;
      for (let gi = gA; gi <= gEnd; gi = gi < gB ? gi + 1 : gEnd) {
        const f = (gi * stepMs - pt) / (t - pt);
        const gx = px + f * (x - px);
        const gy = py + f * (y - py);
        if (segment(sx, sy, gx, gy)) return drawn;
        if (!solid) restart();
        sx = gx;
        sy = gy;
        if (++splits + drawn >= cap) return drawn;
        if (gi === gEnd) break;
      }
      if (segment(sx, sy, x, y)) return drawn;
    } else if (segment(px, py, x, y)) return drawn;
    pt = t != null && Number.isFinite(t) ? t : null;
    px = x;
    py = y;
  }
  return drawn;
}

/** Adds the dash pieces of `xs`/`ys` to `ctx`'s current path, as {@link forEachDashPiece}
 * lays them (clipped to `clip`, at most `cap` pieces, restarting on the `stepMs` grid of
 * the data times `ts` when `stepMs > 0`); a dash running on across a vertex continues the
 * same subpath. The chart's series and its E.D. line both stroke their dashes this way. */
export function traceDashes(
  ctx: Pick<CanvasRenderingContext2D, "moveTo" | "lineTo">,
  xs: ArrayLike<number | null | undefined>,
  ys: ArrayLike<number | null | undefined>,
  pattern: readonly number[],
  clip: DashClip,
  cap: number,
  ts: ArrayLike<number | null | undefined>,
  stepMs: number,
): void {
  let ex = NaN;
  let ey = NaN;
  forEachDashPiece(
    xs,
    ys,
    pattern,
    (x0, y0, x1, y1) => {
      if (x0 !== ex || y0 !== ey) ctx.moveTo(x0, y0);
      ctx.lineTo(x1, y1);
      ex = x1;
      ey = y1;
    },
    clip,
    cap,
    stepMs > 0 ? { ts, stepMs } : undefined,
  );
}
