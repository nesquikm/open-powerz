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

/** A shaded stretch without data, as the chart last drew it. */
export interface ChartGapBand {
  /** The shaded span on screen, session ms. */
  fromMs: number;
  toMs: number;
  label: string;
}

/** A collapsed long gap of the last draw (FR FENCD9, AC-FENCD9.11). */
export interface ChartBreak {
  /** The collapsed gap, session ms. */
  fromMs: number;
  toMs: number;
  /** Its width on screen, CSS px. */
  widthPx: number;
}

/** The overview strip's viewport window, CSS px from the strip's left edge. */
export interface OverviewWindowBox {
  leftPx: number;
  widthPx: number;
}

export interface OpenPowerzE2E {
  /** Points the chart currently has drawn (non-null y values) per series. */
  chartSeriesLengths(): { vbus: number; ibus: number };
  /** Every chart draw so far, oldest first (the last 200 at least). */
  chartDraws(): ChartDraw[];
  /** The time-axis labels of the last draw, left to right. */
  chartXTicks(): string[];
  /** The gaps the last draw shaded. */
  chartGapBands(): ChartGapBand[];
  /** The playhead, session ms, or null when none is placed (FR FENCD9). */
  chartPlayhead(): number | null;
  /** The J/K/L shuttle speed: a signed multiple of real time, 0 when stopped. */
  chartShuttle(): number;
  /** The collapsed gaps of the last draw and their on-screen width. */
  chartBreaks(): ChartBreak[];
  /** The overview strip's viewport window, or null before it is drawn. */
  overviewWindow(): OverviewWindowBox | null;
  /** The collapsed gaps the overview strip drew: on-screen box and length label. */
  overviewBreaks(): { leftPx: number; widthPx: number; label: string }[];
}

export const HOOK = "__OPEN_POWERZ_E2E__";

/**
 * A simulated page reload for the Playwright UI suite (FR VEDDFD), as
 * `window.__OPEN_POWERZ_RELOAD__`, installed only in the mock build
 * (`VITE_BACKEND=mock`). A real `page.reload()` would take the in-page mock
 * backend with it, while the real backend outlives a webview reload; so the
 * hook unmounts the app and mounts a fresh one on the same page: new UI state
 * (session, recording, chart, statistics), the mock backend and its running
 * session untouched, and the old app's event callback left registered until
 * the new app re-attaches. Resolves once the new app is mounted.
 */
export const RELOAD_HOOK = "__OPEN_POWERZ_RELOAD__";

export type ReloadHook = () => Promise<void>;
