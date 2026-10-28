/** The connected meters (FR 1HHT03): one session store per meter, the tile grid and the
 * tile expanded to the full single-meter view. */

import { SvelteMap } from "svelte/reactivity";
import type { BackendEvent, DeviceListing, RecordingSnapshot } from "./backend";
import { Session } from "./session.svelte";

/** A session end the user asked for (a disconnect), not an unplug or a failure. */
function askedToEnd(event: BackendEvent): boolean {
  return (
    event.type === "status" && event.data.state === "disconnected" && event.data.error === null
  );
}

/** One connected meter: its picker entry and its session store. */
export interface MeterEntry {
  listing: DeviceListing;
  session: Session;
}

export class Meters {
  /** Every meter, in the order they were added. */
  #entries = $state.raw<MeterEntry[]>([]);
  /** The tile shown in the full single-meter view, null for the grid. */
  expandedId = $state<string | null>(null);
  /** The meter of the full single-meter view: the id it was connected with, which keeps
   * naming it after a replug moved it to another address; null when the view shows none.
   * It holds through an unplug, the wait and the replug, until the user ends it. */
  fullViewId = $state<string | null>(null);
  /** Recordings of meters that left the full view while recording, by device id. */
  #parked = new SvelteMap<string, RecordingSnapshot>();

  /** The meters' device ids, in the order they were added. */
  get ids(): string[] {
    return this.#entries.map((e) => e.listing.id);
  }

  /** Every meter, in the order they were added. */
  get entries(): readonly MeterEntry[] {
    return this.#entries;
  }

  get(id: string): MeterEntry | undefined {
    return this.#entries.find((e) => e.listing.id === id);
  }

  /** Adds a meter with a store of its own (or `session`); a meter already held keeps its
   * store. */
  add(listing: DeviceListing, session: Session = new Session()): MeterEntry {
    const held = this.get(listing.id);
    if (held) return held;
    const entry = { listing, session };
    this.#entries = [...this.#entries, entry];
    return entry;
  }

  /** Gives meter `id` another session store; one it does not hold is ignored. */
  replace(id: string, session: Session): void {
    this.#entries = this.#entries.map((e) =>
      e.listing.id === id ? { listing: e.listing, session } : e,
    );
  }

  /** Drops a meter, with any recording parked for it; an expanded one goes back to the
   * grid of the rest. */
  remove(id: string): void {
    this.#entries = this.#entries.filter((e) => e.listing.id !== id);
    this.#parked.delete(id);
    if (this.expandedId === id) this.expandedId = null;
  }

  /** Routes a backend event to the store of meter `id`; one it does not hold is ignored. */
  handleEvent(id: string, event: BackendEvent): void {
    this.get(id)?.session.handleEvent(event);
  }

  /** Routes an event of meter `id`, outside the full view, to its tile. A disconnect the
   * user asked for takes the tile away; an unplug keeps it, to show the wait and the replug.
   * A recording that ends with the session, or on its own, leaves nothing parked. */
  handleTileEvent(id: string, event: BackendEvent): void {
    this.handleEvent(id, event);
    const ended =
      (event.type === "recording" &&
        (event.data.state === "stopped" || event.data.state === "failed")) ||
      (event.type === "status" && event.data.state === "disconnected");
    if (ended) this.#parked.delete(id);
    if (askedToEnd(event)) this.remove(id);
  }

  /** Whether meter `id`'s events go to the full view's pipeline. */
  inFullView(id: string): boolean {
    return this.fullViewId !== null && this.fullViewId === id;
  }

  /** Follows the full view's meter through one of its events: a disconnect the user asked
   * for ends it; an unplug keeps it, so the wait and the replug still reach the view. */
  fullViewEvent(event: BackendEvent): void {
    if (askedToEnd(event)) this.fullViewId = null;
  }

  /** Keeps the entry of the full view's meter in step with the view's `session` store: it
   * joins (with `listing`) while that session runs, stays while it waits for its meter, and
   * leaves once the view's meter ended or another meter took the view. */
  syncFullView(session: Session, listing: DeviceListing | undefined): void {
    const id = this.fullViewId;
    if (id !== null && session.active && listing?.id === id) this.add(listing, session);
    for (const e of this.#entries) {
      if (e.session === session && e.listing.id !== id) this.remove(e.listing.id);
    }
  }

  /** Keeps meter `id`'s recording while the meter is away from the full view. */
  park(id: string, recording: RecordingSnapshot): void {
    this.#parked.set(id, recording);
  }

  /** The recording parked for meter `id`, once; undefined when none. */
  unpark(id: string): RecordingSnapshot | undefined {
    const rec = this.#parked.get(id);
    this.#parked.delete(id);
    return rec;
  }

  /** The grid shows for two meters or more, unless one tile is expanded. */
  get showGrid(): boolean {
    return this.#entries.length >= 2 && this.expandedId === null;
  }

  /** Shows meter `id` in the full single-meter view; one it does not hold is refused. */
  expand(id: string): void {
    if (this.get(id)) this.expandedId = id;
  }

  /** Back to the grid. */
  collapse(): void {
    this.expandedId = null;
  }

  /** The meter the full single-meter view shows: the expanded one, or the only one. */
  get focused(): MeterEntry | undefined {
    if (this.expandedId !== null) return this.get(this.expandedId);
    return this.#entries.length === 1 ? this.#entries[0] : undefined;
  }
}

/** Copies the live state of one meter's session store into another, when a meter moves
 * into or out of the full single-meter view (FR 1HHT03). */
export function copyLive(from: Session, to: Session): void {
  to.selectedId = from.selectedId;
  to.state = from.state;
  to.error = from.error;
  to.latest = from.latest;
  to.tempC = from.tempC;
  to.rateSps = from.rateSps;
  to.stats = from.stats;
  to.details = from.details;
  to.waitingSerial = from.waitingSerial;
}
