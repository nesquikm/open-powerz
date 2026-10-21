/**
 * The e2e test hook (FR AH2AEN): `window.__OPEN_POWERZ_E2E__`, installed only in a
 * test build (`VITE_E2E=1` or `VITE_BACKEND=mock`). Callers guard every use with
 * {@link E2E_BUILD}, a build-time constant, so a production build drops this module.
 */

import { HOOK, type ChartDraw, type OpenPowerzE2E } from "../../e2e/hook";
import type { ChartFrame } from "./chartData";

/** Whether this web build is a test build that exposes the hook. */
export const E2E_BUILD: boolean =
  import.meta.env.VITE_E2E === "1" || import.meta.env.VITE_BACKEND === "mock";

/** Draws kept for the hook (at least the last 200). */
const MAX_DRAWS = 1000;

/** Records the chart's draws and serves them through the hook. */
export class ChartProbe {
  #frame: ChartFrame | null = null;
  #draws: ChartDraw[] = [];

  /** One finished draw of `frame`. */
  record(frame: ChartFrame, draw: Omit<ChartDraw, "at">): void {
    this.#frame = frame;
    this.#draws.push({ at: performance.now(), ...draw });
    if (this.#draws.length > MAX_DRAWS) this.#draws.splice(0, this.#draws.length - MAX_DRAWS);
  }

  /** Non-null points of one series in the frame on screen. */
  #drawn(key: "vbus" | "ibus"): number {
    const ys = this.#frame?.y[key] ?? [];
    let n = 0;
    for (const v of ys) if (v !== null && v !== undefined) n++;
    return n;
  }

  /** The hook object over this probe. */
  hook(): OpenPowerzE2E {
    return {
      chartSeriesLengths: () => ({ vbus: this.#drawn("vbus"), ibus: this.#drawn("ibus") }),
      chartDraws: () => this.#draws.map((d) => ({ ...d })),
    };
  }
}

/** Installs the hook for `probe`; returns the uninstall. */
export function installChartProbe(probe: ChartProbe): () => void {
  const w = window as unknown as Record<string, OpenPowerzE2E | undefined>;
  const hook = probe.hook();
  w[HOOK] = hook;
  return () => {
    if (w[HOOK] === hook) delete w[HOOK];
  };
}
