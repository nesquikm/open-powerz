/**
 * AC-KQF5GM.10: the state a backfilled live window must be in, independent of how long
 * the runner took to get there. A window of `windowMs` starts at the session start, or,
 * once the session is longer than the window, it is full and ends at the newest sample:
 * the expected start is max(session start, newest - window length).
 */

/** One backfill step: how far the drawn start may sit from the expected start. */
export const LIVE_START_TOLERANCE_MS = 250;

export interface LiveWindowState {
  /** Oldest session ms the live draw covers. */
  t0Ms: number;
  /** Newest session ms the live draw covers. */
  t1Ms: number;
  /** The live window's length. */
  windowMs: number;
  /** Session ms recorded before the reload; the window must reach back this far
   * (capped at the window length). */
  reachBackMs: number;
  /** Session start in session ms (default 0). */
  sessionStartMs?: number;
}

/** Why a live draw is not a correctly backfilled window, or `null` when it is. */
export function liveWindowProblem({
  t0Ms,
  t1Ms,
  windowMs,
  reachBackMs,
  sessionStartMs = 0,
}: LiveWindowState): string | null {
  if (![t0Ms, t1Ms, windowMs, reachBackMs, sessionStartMs].every(Number.isFinite))
    return `non-finite range ${t0Ms}..${t1Ms} ms`;
  if (t1Ms < t0Ms) return `ends before it starts (${t0Ms}..${t1Ms} ms)`;

  const expectedStart = Math.max(sessionStartMs, t1Ms - windowMs);
  if (Math.abs(t0Ms - expectedStart) > LIVE_START_TOLERANCE_MS)
    return `starts at ${t0Ms} ms, expected ${expectedStart} ms`;

  const span = t1Ms - t0Ms;
  const needed = Math.min(reachBackMs, windowMs);
  if (span + LIVE_START_TOLERANCE_MS < needed) return `spans ${span} ms, less than ${needed} ms`;
  return null;
}
