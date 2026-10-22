/**
 * The overview strip's per-column envelope: where samples are sparser than one per pixel
 * column, the columns between two samples are filled along the line joining them, and
 * adjacent columns whose ranges do not meet are extended until they touch, so a sparse
 * series reads as a continuous line (AC-SX3EDV.6). No column a gap or break covers is
 * filled, and nothing is joined across one.
 */

/**
 * Bridges the per-column envelope `lo` / `hi` in place. An empty column has
 * `lo = +Infinity`, `hi = -Infinity`. `blocked(c)` is true for every column a marked gap
 * or break covers: such a column is left as it is and ends the line on either side.
 * Leading and trailing empty columns stay empty.
 */
export function bridgeColumns(
  lo: Float64Array,
  hi: Float64Array,
  blocked: (col: number) => boolean,
): void {
  const cols = Math.min(lo.length, hi.length);
  /** The last filled column the line can continue from, or -1. */
  let p = -1;
  for (let c = 0; c < cols; c++) {
    if (blocked(c)) {
      p = -1;
      continue;
    }
    // Empty, or holding a non-finite sample: nothing to draw a line from.
    if (!(hi[c]! >= lo[c]!) || !Number.isFinite(lo[c]!) || !Number.isFinite(hi[c]!)) continue;
    if (p >= 0) {
      if (c === p + 1) {
        // Neighbours with disjoint ranges meet halfway.
        if (lo[c]! > hi[p]!) {
          const mid = (hi[p]! + lo[c]!) / 2;
          hi[p] = mid;
          lo[c] = mid;
        } else if (hi[c]! < lo[p]!) {
          const mid = (lo[p]! + hi[c]!) / 2;
          lo[p] = mid;
          hi[c] = mid;
        }
      } else {
        // Empty columns between: follow the line from p's middle to c's middle, each
        // column covering half a column either side so neighbours touch.
        const vp = (lo[p]! + hi[p]!) / 2;
        const vc = (lo[c]! + hi[c]!) / 2;
        const step = (vc - vp) / (c - p);
        extend(lo, hi, p, vp + step / 2);
        for (let k = p + 1; k < c; k++) {
          const a = vp + step * (k - p - 0.5);
          const b = vp + step * (k - p + 0.5);
          lo[k] = Math.min(a, b);
          hi[k] = Math.max(a, b);
        }
        extend(lo, hi, c, vc - step / 2);
      }
    }
    p = c;
  }
}

/** Widens column `c` to include `v`. */
function extend(lo: Float64Array, hi: Float64Array, c: number, v: number): void {
  if (v < lo[c]!) lo[c] = v;
  if (v > hi[c]!) hi[c] = v;
}
