/**
 * AC-KQF5GM.8: how much the view's span may grow between the draw a test read and the
 * pan it made. The live window widens by at most one session ms per real ms, so the
 * bound is the real time that passed plus a small fixed slack for rounding, not a fixed
 * allowance that a slow runner can outlast.
 */

/** Fixed slack (ms) on top of the elapsed time, for rounding. */
export const SPAN_GROWTH_SLACK_MS = 50;

/**
 * Largest span growth (ms) allowed between a draw at `drawnAt` and a pan at `pannedAt`,
 * both on the page's `performance.now()` clock.
 */
export function maxSpanGrowthMs(drawnAt: number, pannedAt: number): number {
  if (!Number.isFinite(drawnAt) || !Number.isFinite(pannedAt)) {
    throw new Error(`non-finite time: drawnAt=${drawnAt}, pannedAt=${pannedAt}`);
  }
  if (pannedAt < drawnAt) {
    throw new Error(`pan read at ${pannedAt} ms, before the draw at ${drawnAt} ms`);
  }
  return pannedAt - drawnAt + SPAN_GROWTH_SLACK_MS;
}
