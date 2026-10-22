/**
 * The PD view's capture state (FR TGE83C): takes the backend's `pd_state` / `pd_rows`
 * events off the session channel, keeps the rows in a {@link PdStore}, and says when
 * Start and Stop are available and, when they are not, why.
 */

import type { BackendEvent, PdRow } from "./backend";
import { PdStore } from "./pdStore";

/** Start and Stop availability. */
export interface PdControls {
  canStart: boolean;
  canStop: boolean;
  /** Why both are unavailable, or null. */
  reason: string | null;
}

/** Why Start and Stop are disabled without a meter. */
export const PD_NO_METER_REASON = "Connect a meter to capture PD traffic.";

export class PdCapture {
  /** The PD sniffer runs. */
  capturing = $state(false);
  /** Every captured row (not reactive itself: watch {@link version}). */
  readonly store = new PdStore();
  /** Bumped on every change to the store. */
  version = $state(0);
  /** Bumped when the store is emptied (a clear, a new session): row indices start
   * over, so a view drops its selection. */
  emptied = $state(0);
  /** Rows numbered below this were sent before the last clear: dropped. */
  #from = 0;
  /** Rows that arrived while a clear waits for the backend's answer, or null. */
  #held: PdRow[] | null = null;
  /** Numbers the clears, so a late answer to an abandoned one changes nothing. */
  #ticket = 0;

  /** Start / Stop availability with the meter `connected` or not. */
  controls(connected: boolean): PdControls {
    if (!connected) return { canStart: false, canStop: false, reason: PD_NO_METER_REASON };
    return { canStart: !this.capturing, canStop: this.capturing, reason: null };
  }

  /** Takes the PD events of the session channel; a session that ends stops the capture. */
  handleEvent(event: BackendEvent): void {
    switch (event.type) {
      case "pd_state":
        this.capturing = event.data.capturing;
        break;
      case "pd_rows": {
        const from = this.#from;
        const rows = event.data.rows.filter((r) => r.index >= from);
        if (rows.length === 0) break;
        if (this.#held) {
          this.#held.push(...rows);
          break;
        }
        this.store.append(rows);
        this.version++;
        break;
      }
      case "status":
        if (event.data.state === "disconnected" || event.data.state === "waiting") {
          this.capturing = false;
        } else if (event.data.state === "connected") {
          // A new backend session numbers its records from 0 again; the old
          // session's capture ended with it.
          this.#ticket++;
          this.#held = null;
          this.#from = 0;
          this.store.clear();
          this.emptied++;
          this.version++;
        }
        break;
      default:
        break;
    }
  }

  /** Empties the store; a running capture goes on. `from` is the backend's next
   * record index: rows numbered below it that arrive later are stale. */
  clear(from = 0): void {
    this.endClear(from, this.beginClear());
  }

  /** First half of a clear, as it is asked of the backend: empties the store now and
   * holds the rows that arrive until {@link endClear}. Returns the clear's ticket. */
  beginClear(): number {
    this.#held = [];
    this.store.clear();
    this.emptied++;
    this.version++;
    return ++this.#ticket;
  }

  /** Second half of a clear, with the backend's next record index `from` (0 when the
   * backend did not answer): keeps the held rows numbered `from` or more. A ticket
   * other than the latest clear's (a newer clear, or a new session) changes nothing. */
  endClear(from: number, ticket = this.#ticket): void {
    const held = this.#held;
    if (held === null || ticket !== this.#ticket) return;
    this.#held = null;
    this.#from = Math.max(this.#from, from);
    const rows = held.filter((r) => r.index >= this.#from);
    if (rows.length === 0) return;
    this.store.append(rows);
    this.version++;
  }
}
