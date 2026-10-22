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
  return env.CI ? RENDER_STEP_BUDGET_MS * CI_FACTOR : RENDER_STEP_BUDGET_MS;
}

/** Failure message naming the step, its render times and the budget it was held to. */
export function renderReport(step: string, timings: readonly number[], budgetMs: number): string {
  return `${step} render times (ms): ${timings.map((t) => t.toFixed(1)).join(", ")}; budget ${budgetMs} ms`;
}
