/**
 * Segment marks and E.D. from the backend's raw samples (FR AXHK2T, AC-AXHK2T.4, .5).
 * A chart says on every draw what it wants; this asks `run_progress` for it with one
 * request in flight, the newest want winning, and at most one refresh per
 * `minIntervalMs` while only the data or the window moves. An option change asks at
 * once. The drawn arrays are refilled in place: WebKit keeps dropped arrays alive
 * (AC-4NSVC9.2).
 */

import type { RunProgress, RunProgressQuery } from "./backend";
import type { TimeRange } from "./chartView";
import type { EnergySharePoints, MarkKind, SegmentMark } from "./chartExtras";

/** What a chart wants drawn. */
export interface ProgressWant {
  marks: MarkKind;
  step: number;
  ed: boolean;
  /** E.D. points across the window, about the plot width. */
  edPoints: number;
  edT0Ms: number | null;
  edT1Ms: number | null;
  /** Changes whenever the samples may have changed (the newest sample's time, say). */
  stamp: number;
}

export type RunProgressFetch = (q: RunProgressQuery) => Promise<RunProgress | null>;

export interface RunProgressFeedOptions {
  /** Least time between two refreshes of unchanged options, ms (about 4 Hz). */
  minIntervalMs?: number;
  now?: () => number;
}

/** The options part of a want: a change here asks at once. */
function optionsKey(w: ProgressWant): string {
  return `${w.marks}|${w.step}|${w.ed}`;
}

function sameWant(a: ProgressWant | null, b: ProgressWant): boolean {
  return (
    a !== null &&
    optionsKey(a) === optionsKey(b) &&
    a.edPoints === b.edPoints &&
    a.edT0Ms === b.edT0Ms &&
    a.edT1Ms === b.edT1Ms &&
    a.stamp === b.stamp
  );
}

export class RunProgressFeed {
  /** The marks to draw, refilled in place. */
  readonly marks: SegmentMark[] = [];
  /** The E.D. points to draw, refilled in place. */
  readonly ed: EnergySharePoints = { tMs: [], pct: [] };
  /** Whether the run's followed total is above zero; null while unknown or marks off. */
  marksTotalPositive: boolean | null = null;
  /** Whether the session's energy is above zero; null while unknown or E.D. off. */
  energyTotalPositive: boolean | null = null;
  /** The first and the newest sample's time, from the last answer. */
  extent: TimeRange | null = null;

  #fetch: RunProgressFetch;
  #onAnswer: () => void;
  #minIntervalMs: number;
  #now: () => number;
  #wanted: ProgressWant | null = null;
  #sent: ProgressWant | null = null;
  #sentAt = -Infinity;
  #inFlight = false;
  #timer: ReturnType<typeof setTimeout> | null = null;
  #generation = 0;

  constructor(fetch: RunProgressFetch, onAnswer: () => void, options: RunProgressFeedOptions = {}) {
    this.#fetch = fetch;
    this.#onAnswer = onAnswer;
    this.#minIntervalMs = options.minIntervalMs ?? 250;
    this.#now = options.now ?? (() => performance.now());
  }

  /** What to draw now; null for neither marks nor E.D. Cheap to call on every draw. */
  want(w: ProgressWant | null): void {
    const on = w !== null && (w.marks !== "off" || w.ed);
    const next = on ? w : null;
    if (next === null || next.marks === "off") this.#clearMarks();
    if (next === null || !next.ed) this.#clearEd();
    const before = this.#wanted;
    if (next !== null && before !== null && optionsKey(before) !== optionsKey(next)) {
      // Another kind or step: the marks drawn are for something else now.
      if (before.marks !== next.marks || before.step !== next.step) this.#clearMarks();
    }
    this.#wanted = next;
    if (next === null) {
      this.#cancelTimer();
      return;
    }
    this.#kick();
  }

  /** Forgets everything; an answer still on its way is dropped. */
  clear(): void {
    this.#generation++;
    this.#wanted = null;
    this.#sent = null;
    this.#sentAt = -Infinity;
    this.#cancelTimer();
    this.#clearMarks();
    this.#clearEd();
    this.extent = null;
  }

  /** Stops the trailing refresh (the chart is gone). */
  dispose(): void {
    this.clear();
  }

  #clearMarks(): void {
    this.marks.length = 0;
    this.marksTotalPositive = null;
  }

  #clearEd(): void {
    this.ed.tMs.length = 0;
    this.ed.pct.length = 0;
    this.energyTotalPositive = null;
  }

  #cancelTimer(): void {
    if (this.#timer !== null) clearTimeout(this.#timer);
    this.#timer = null;
  }

  /** Sends the wanted query when nothing is in flight and the gate allows. */
  #kick(): void {
    const w = this.#wanted;
    if (w === null || this.#inFlight || sameWant(this.#sent, w)) return;
    const optionsChanged = this.#sent === null || optionsKey(this.#sent) !== optionsKey(w);
    const wait = optionsChanged ? 0 : this.#sentAt + this.#minIntervalMs - this.#now();
    if (wait > 0) {
      if (this.#timer === null) {
        this.#timer = setTimeout(() => {
          this.#timer = null;
          this.#kick();
        }, wait);
      }
      return;
    }
    this.#cancelTimer();
    void this.#send(w);
  }

  async #send(w: ProgressWant): Promise<void> {
    const generation = this.#generation;
    this.#sent = w;
    this.#sentAt = this.#now();
    this.#inFlight = true;
    let a: RunProgress | null;
    try {
      a = await this.#fetch({
        kind: w.marks === "energy" ? "energy" : "charge",
        step_pct: w.step,
        ed_points: w.ed ? w.edPoints : 0,
        ed_t0_ms: w.edT0Ms,
        ed_t1_ms: w.edT1Ms,
        ed_scope: "session",
      });
    } catch (e) {
      console.warn("open-powerz: run progress failed", e);
      // The same want is not asked again; the next change of data or view asks anew.
      a = null;
    } finally {
      this.#inFlight = false;
    }
    const now = this.#wanted;
    if (generation === this.#generation && now !== null && optionsKey(now) === optionsKey(w)) {
      this.#apply(a, now);
      this.#onAnswer();
    }
    if (generation === this.#generation) this.#kick();
  }

  #apply(a: RunProgress | null, w: ProgressWant): void {
    this.extent = a === null ? this.extent : { t0Ms: a.session_t0_ms, t1Ms: a.run_t1_ms };
    if (w.marks !== "off") {
      const marks = a?.marks ?? [];
      for (let k = 0; k < marks.length; k++) {
        const m = marks[k]!;
        const label = `${m.percent}%`;
        const old = this.marks[k];
        if (old === undefined) this.marks.push({ tMs: m.t_ms, percent: m.percent, label });
        else {
          old.tMs = m.t_ms;
          old.percent = m.percent;
          old.label = label;
        }
      }
      this.marks.length = marks.length;
      this.marksTotalPositive = a === null ? null : a.marks_total_positive;
    }
    if (w.ed) {
      const t = a?.ed_t_ms ?? [];
      const p = a?.ed_pct ?? [];
      const n = Math.min(t.length, p.length);
      for (let k = 0; k < n; k++) {
        this.ed.tMs[k] = t[k]!;
        this.ed.pct[k] = p[k]!;
      }
      this.ed.tMs.length = n;
      this.ed.pct.length = n;
      this.energyTotalPositive = a === null ? null : a.energy_total_positive;
    }
  }
}
