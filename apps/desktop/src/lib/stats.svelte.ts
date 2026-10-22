import { toUiError, type UiError } from "./backend";

/** The backend calls the stats controls need (injected, so tests can fake them). */
export interface StatsDeps {
  resetStats: () => Promise<void>;
  clearHistory: () => Promise<void>;
  /** Empties what the chart shows; called only after the backend cleared its history. */
  clearChart?: () => void;
}

/** Reset-stats and clear-history controls. Clearing the history throws data away, so
 * it only happens on `confirmClear` after a `requestClear`. */
export class StatsControls {
  /** A clear was requested and waits for confirmation. */
  clearPending = $state(false);
  /** The last refused reset or clear, or `null`. */
  error = $state<UiError | null>(null);

  /** Zero the statistics (R shortcut / Reset button); resolves to whether it went through. */
  async reset(deps: StatsDeps): Promise<boolean> {
    this.error = null;
    try {
      await deps.resetStats();
      return true;
    } catch (err) {
      this.error = toUiError(err);
      return false;
    }
  }

  /** Ask for confirmation before clearing the history. */
  requestClear(): void {
    this.clearPending = true;
  }

  /** Drop the pending clear request. */
  cancelClear(): void {
    this.clearPending = false;
  }

  /** Clear the history, only if a clear was requested; uses the request up. */
  async confirmClear(deps: StatsDeps): Promise<void> {
    if (!this.clearPending) return;
    this.clearPending = false;
    this.error = null;
    try {
      await deps.clearHistory();
    } catch (err) {
      this.error = toUiError(err);
      return;
    }
    deps.clearChart?.();
  }
}
