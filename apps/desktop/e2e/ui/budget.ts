/**
 * AC-KQF5GM.5: the render-step budget of the pan, overview-drag, wheel-zoom and shuttle
 * tests. Shared CI runners are slower than a dev machine, so the budget is widened by a
 * fixed factor when `CI` is set; every failure message names the budget it used.
 */

/** Local render-step budget (ms). */
export const RENDER_STEP_BUDGET_MS = 100;

/** Factor the budget is widened by on CI. */
export const CI_FACTOR = 3;

/** Render-step budget (ms) for `env`: the local budget, times `CI_FACTOR` when `CI` is set. */
export function renderStepBudgetMs(env: NodeJS.ProcessEnv = process.env): number {
  return ciLatencyBudgetMs(RENDER_STEP_BUDGET_MS, env);
}

/** A latency budget (ms) for `env`: kept locally, times `CI_FACTOR` when `CI` is set. */
export function ciLatencyBudgetMs(budgetMs: number, env: NodeJS.ProcessEnv = process.env): number {
  return env.CI ? budgetMs * CI_FACTOR : budgetMs;
}

/** A rate floor (per second) for `env`: kept locally, divided by `CI_FACTOR` when `CI` is set. */
export function ciRateFloor(floorPerSecond: number, env: NodeJS.ProcessEnv = process.env): number {
  return env.CI ? floorPerSecond / CI_FACTOR : floorPerSecond;
}

/** AC-KQF5GM.9: lower bound (per second) of the 1000 SPS live redraw rate. */
export const LIVE_REDRAW_MIN_PER_S = 20;

/** AC-KQF5GM.9: upper bound (per second) of the 1000 SPS live redraw rate. */
export const LIVE_REDRAW_MAX_PER_S = 60;

/**
 * AC-KQF5GM.9: the live redraw band for `env`. Locally 20–60 per second; on CI only the
 * lower bound is divided by `CI_FACTOR`, the upper bound stays.
 */
export function liveRedrawBand(env: NodeJS.ProcessEnv = process.env): {
  minPerSecond: number;
  maxPerSecond: number;
} {
  return {
    minPerSecond: ciRateFloor(LIVE_REDRAW_MIN_PER_S, env),
    maxPerSecond: LIVE_REDRAW_MAX_PER_S,
  };
}

/** Failure message naming the step, its render times and the budget it was held to. */
export function renderReport(step: string, timings: readonly number[], budgetMs: number): string {
  return `${step} render times (ms): ${timings.map((t) => t.toFixed(1)).join(", ")}; budget ${budgetMs} ms`;
}

/** AC-2BF5G6.5: local pan-step budget (ms) of the 10-million-sample history test. */
export const HISTORY_PAN_BUDGET_MS = 100;

/** AC-2BF5G6.5: local test timeout (ms) of the 10-million-sample history test. */
export const HISTORY_PAN_TIMEOUT_MS = 5_000;

/**
 * AC-2BF5G6.5: pan budget and test timeout of the 10-million-sample history test for
 * `env`; both kept locally and widened by `CI_FACTOR` when `CI` is set.
 */
export function historyPanBudget(env: NodeJS.ProcessEnv = process.env): {
  panBudgetMs: number;
  timeoutMs: number;
} {
  return {
    panBudgetMs: ciLatencyBudgetMs(HISTORY_PAN_BUDGET_MS, env),
    timeoutMs: ciLatencyBudgetMs(HISTORY_PAN_TIMEOUT_MS, env),
  };
}
