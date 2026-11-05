/**
 * A chart's ruler (FR AXHK2T): where its cursors are and the backend's answer for the
 * span between them. One `range_stats` call in flight at most; the newest span wins.
 * While the answer for the cursors as they are now is on its way, the readout says so.
 * The screen reader hears the values once they settle, not on every step (AC-AXHK2T.3).
 */

import type { RangeStats } from "./backend";
import {
  placeRuler,
  rulerAnnouncement,
  rulerReadout,
  rulerSpan,
  rulerValues,
  type Ruler,
  type RulerReadoutItem,
} from "./ruler";

/** Asks the backend for the stats between two times. */
export type RangeStatsFetch = (t0Ms: number, t1Ms: number) => Promise<RangeStats | null>;

/** Quiet time after the last move before the values are announced, ms. */
export const RULER_SETTLE_MS = 500;

export class RulerState {
  /** The cursors, or null while the ruler is not placed. */
  ruler: Ruler | null = $state(null);
  /** The backend's last answer, or null. */
  stats: RangeStats | null = $state(null);
  /** The answer for the cursors as they are now is still on its way. */
  pending = $state(false);
  /** What the polite live region says: the values, once they settle. */
  announcement = $state("");
  /** The six readout values. */
  readonly readout: RulerReadoutItem[] = $derived(
    this.ruler === null ? [] : rulerReadout(rulerValues(this.ruler, this.stats), this.pending),
  );

  #fetch: RangeStatsFetch;
  #pendingSpan: Ruler | null = null;
  #inFlight = false;
  /** Bumped by {@link clear}, so an answer for an earlier ruler is dropped. */
  #generation = 0;
  #settleTimer: ReturnType<typeof setTimeout> | null = null;
  /** Announce as soon as the answer is in. */
  #announceOnAnswer = false;

  constructor(fetch: RangeStatsFetch) {
    this.#fetch = fetch;
  }

  /** Moves the cursors to `r` and asks for its stats. */
  set(r: Ruler): void {
    this.ruler = r;
    this.pending = true;
    this.#pendingSpan = r;
    this.#announceOnAnswer = false;
    this.#armSettle();
    if (!this.#inFlight) void this.#pump();
  }

  /** With no ruler placed, places one in the view `[t0Ms, t1Ms]`; an empty view places none. */
  placeIn(t0Ms: number, t1Ms: number): void {
    if (this.ruler !== null || !(t1Ms > t0Ms)) return;
    this.set(placeRuler({ t0Ms, t1Ms }));
  }

  /** Puts both cursors back in the view `[t0Ms, t1Ms]` ("Reset ruler"). */
  reset(t0Ms: number, t1Ms: number): void {
    if (!(t1Ms > t0Ms)) return;
    this.set(placeRuler({ t0Ms, t1Ms }));
  }

  /** A key or the pointer was released: announce once the answer is in. */
  settle(): void {
    if (this.ruler === null) return;
    this.#cancelSettle();
    if (this.pending) this.#announceOnAnswer = true;
    else this.#announce();
  }

  /** Removes the ruler. */
  clear(): void {
    this.ruler = null;
    this.stats = null;
    this.pending = false;
    this.announcement = "";
    this.#pendingSpan = null;
    this.#announceOnAnswer = false;
    this.#cancelSettle();
    this.#generation++;
  }

  #announce(): void {
    this.announcement = rulerAnnouncement(this.readout);
  }

  #armSettle(): void {
    this.#cancelSettle();
    this.#settleTimer = setTimeout(() => {
      this.#settleTimer = null;
      this.settle();
    }, RULER_SETTLE_MS);
  }

  #cancelSettle(): void {
    if (this.#settleTimer !== null) clearTimeout(this.#settleTimer);
    this.#settleTimer = null;
  }

  async #pump(): Promise<void> {
    this.#inFlight = true;
    try {
      while (this.#pendingSpan !== null) {
        const generation = this.#generation;
        const span = rulerSpan(this.#pendingSpan);
        this.#pendingSpan = null;
        let answer: RangeStats | null;
        try {
          answer = await this.#fetch(span.t0Ms, span.t1Ms);
        } catch (e) {
          console.warn("open-powerz: ruler stats failed", e);
          answer = null;
        }
        // A newer span is on its way: this answer is already stale.
        if (generation === this.#generation && this.#pendingSpan === null) {
          this.stats = answer;
          this.pending = false;
          if (this.#announceOnAnswer) {
            this.#announceOnAnswer = false;
            this.#announce();
          }
        }
      }
    } finally {
      this.#inFlight = false;
    }
  }
}
