/**
 * The chart options (FR AXHK2T) in one place: the live chart and an opened recording read
 * and change the same value, which is saved on every change (AC-AXHK2T.8).
 */

import { loadChartExtras, saveChartExtras, type ChartExtras } from "./chartExtras";
import { browserStorage } from "./theme";

class ChartExtrasStore {
  /** The options now, as last saved. */
  value: ChartExtras = $state(loadChartExtras(browserStorage()));

  /** Changes some options and saves them all. */
  update(patch: Partial<ChartExtras>): void {
    this.value = { ...this.value, ...patch };
    saveChartExtras(this.value, browserStorage());
  }
}

/** The one store of the chart options. */
export const chartExtras = new ChartExtrasStore();
