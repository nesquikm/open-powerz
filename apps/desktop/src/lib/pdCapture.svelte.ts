/**
 * The PD view's capture state (FR TGE83C): takes the backend's `pd_state` / `pd_rows`
 * events off the session channel, keeps the rows in a {@link PdStore}, and says when
 * Start and Stop (and Open capture / Import vendor log) are available and, when they
 * are not, why.
 */

import type { BackendEvent, PdRow, PdStatusSample } from "./backend";
import { PdStore } from "./pdStore";
import { PdStatusBuffer } from "./pdTimeline";

/** Start and Stop availability. */
export interface PdControls {
  canStart: boolean;
  canStop: boolean;
  /** Why both are unavailable, or null. */
  reason: string | null;
  /** Open capture and Import vendor log are available (FR TGE83E). */
  canOpen: boolean;
  /** Why they are not, or null. */
  openReason: string | null;
}

/** Why Start and Stop are disabled without a meter. */
export const PD_NO_METER_REASON = "Connect a meter to capture PD traffic.";

/** Why Open capture and Import vendor log are disabled while capturing: the live rows
 * would go on arriving behind the opened file, unseen. */
export const PD_STOP_FIRST_REASON = "Stop the capture first to open or import a file.";

export class PdCapture {
  /** The PD sniffer runs. */
  capturing = $state(false);
  /** Every captured row (not reactive itself: watch {@link version}). */
  readonly store = new PdStore();
  /** Bumped on every change to the store. */
  version = $state(0);
  /** The PD status blocks of the session (FR TGE83D; not reactive: watch
   * {@link statusVersion}). */
  readonly status = new PdStatusBuffer();
  /** Bumped on every change to {@link status}. */
  statusVersion = $state(0);
  /** Bumped when the store is emptied (a clear, a new session): row indices start
   * over, so a view drops its selection. */
  emptied = $state(0);
  /** The view shows a capture opened from a file (FR TGE83E): the live capture's rows
   * and status blocks stay out of it until a clear or a new session. */
  opened = $state(false);
  /** Rows numbered below this were sent before the last clear: dropped. */
  #from = 0;

  /** Index of the table's first row: an opened file numbers its rows from 0, the live
   * capture from its last clear. Rows are numbered on screen from here. */
  get firstIndex(): number {
    return this.opened ? 0 : this.#from;
  }
  /** Rows that arrived while a clear waits for the backend's answer, or null. */
  #held: PdRow[] | null = null;
  /** Numbers the clears, so a late answer to an abandoned one changes nothing. */
  #ticket = 0;

  /** Start / Stop availability with the meter `connected` or not. */
  controls(connected: boolean): PdControls {
    const live = connected && this.capturing;
    const open = { canOpen: !live, openReason: live ? PD_STOP_FIRST_REASON : null };
    if (!connected) return { canStart: false, canStop: false, reason: PD_NO_METER_REASON, ...open };
    return { canStart: !this.capturing, canStop: this.capturing, reason: null, ...open };
  }

  /** Takes the PD events of the session channel; a session that ends stops the capture. */
  handleEvent(event: BackendEvent): void {
    switch (event.type) {
      case "pd_state":
        this.capturing = event.data.capturing;
        break;
      case "pd_rows": {
        if (this.opened) break;
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
      case "pd_status":
        if (this.opened || event.data.samples.length === 0) break;
        this.status.append(event.data.samples);
        this.statusVersion++;
        break;
      case "status":
        if (event.data.state === "disconnected" || event.data.state === "waiting") {
          this.capturing = false;
        } else if (event.data.state === "connected") {
          // A new backend session numbers its records from 0 again; the old
          // session's capture ended with it.
          this.#ticket++;
          this.#held = null;
          this.#from = 0;
          this.opened = false;
          this.store.clear();
          this.emptied++;
          this.version++;
          this.status.clear();
          this.statusVersion++;
        }
        break;
      default:
        break;
    }
  }

  /** Shows a capture opened from a file in place of the live one: its `rows` (numbered
   * from 0) and `status` samples. A pending clear is abandoned. */
  open(rows: PdRow[], status: PdStatusSample[]): void {
    this.#ticket++;
    this.#held = null;
    this.opened = true;
    this.store.clear();
    this.store.append(rows);
    this.emptied++;
    this.version++;
    this.status.clear();
    this.status.append(status);
    this.statusVersion++;
  }

  /** Empties the store; a running capture goes on. `from` is the backend's next
   * record index: rows numbered below it that arrive later are stale. */
  clear(from = 0): void {
    this.endClear(from, this.beginClear());
  }

  /** First half of a clear, as it is asked of the backend: empties the store now and
   * holds the rows that arrive until {@link endClear}. Returns the clear's ticket. */
  beginClear(): number {
    this.opened = false;
    this.#held = [];
    this.store.clear();
    this.status.clear();
    this.statusVersion++;
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
