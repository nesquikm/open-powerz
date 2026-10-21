/**
 * The test hook both end-to-end suites read (FR AH2AEN).
 *
 * The web build installs it as `window.__OPEN_POWERZ_E2E__` only when it is
 * built for a test: `VITE_E2E=1` (the real-app build that goes with the `e2e`
 * Cargo feature) or `VITE_BACKEND=mock` (the Playwright UI build). A normal
 * production build must not expose it.
 */

/** One draw of the chart, as the hook reports it. */
export interface ChartDraw {
  /** `performance.now()` when the draw finished. */
  at: number;
  /** Live (scrolling) window or a fixed history range. */
  mode: "live" | "history";
  /** The drawn x range, session ms. */
  t0Ms: number;
  t1Ms: number;
  /** The data came from a history response for exactly this range (not the live buffer). */
  fromHistory: boolean;
}

export interface OpenPowerzE2E {
  /** Points the chart currently has drawn (non-null y values) per series. */
  chartSeriesLengths(): { vbus: number; ibus: number };
  /** Every chart draw so far, oldest first (the last 200 at least). */
  chartDraws(): ChartDraw[];
}

export const HOOK = "__OPEN_POWERZ_E2E__";
