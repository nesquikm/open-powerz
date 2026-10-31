/** The connected meters (FR 1HHT03): one session store per meter, the tile grid and the
 * tile expanded to the full single-meter view. */

import { SvelteMap } from "svelte/reactivity";
import type { BackendEvent, DeviceListing, RecordingSnapshot } from "./backend";
import type { PdCapture } from "./pdCapture.svelte";
import { ExportJob, Recording } from "./recording.svelte";
import { Session } from "./session.svelte";

/** A session end the user asked for (a disconnect), not an unplug or a failure. */
function askedToEnd(event: BackendEvent): boolean {
  return (
    event.type === "status" && event.data.state === "disconnected" && event.data.error === null
  );
}

/** One connected meter: its picker entry, its session store, and its own recording and
 * export while it is outside the full view (the full view's meter uses the app's). */
export interface MeterEntry {
  listing: DeviceListing;
  session: Session;
  recording: Recording;
  exportJob: ExportJob;
}

export class Meters {
  /** Every meter, in the order they were added (or {@link arrange}d). */
  #entries = $state.raw<MeterEntry[]>([]);
  /** The PD capture of each meter that left the full view with one running (AC-4PJRVM.4). */
  #parkedPd = new SvelteMap<string, PdCapture>();
  /** The order a reload found the meters in, for the ones still to join (AC-4PJRVM.3). */
  #order: string[] = [];
  /** The tile shown in the full single-meter view, null for the grid. */
  expandedId = $state<string | null>(null);
  /** The meter of the full single-meter view: the id it was connected with, which keeps
   * naming it after a replug moved it to another address; null when the view shows none.
   * It holds through an unplug, the wait and the replug, until the user ends it. */
  fullViewId = $state<string | null>(null);

  /** The meters' device ids, in tile order. */
  get ids(): string[] {
    return this.#entries.map((e) => e.listing.id);
  }

  /** Every meter, in tile order. */
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
    const entry = { listing, session, recording: new Recording(), exportJob: new ExportJob() };
    this.#entries = this.#arranged([...this.#entries, entry]);
    return entry;
  }

  /** Puts the meters in the backend's connection order `ids`, whichever a reload attaches
   * first (AC-4PJRVM.3): the ones held now, and the ones that join later. */
  arrange(ids: string[]): void {
    this.#order = ids;
    this.#entries = this.#arranged(this.#entries);
  }

  /** `entries` in the {@link arrange}d order; a meter outside it keeps its place at the end. */
  #arranged(entries: MeterEntry[]): MeterEntry[] {
    const rank = (e: MeterEntry) => {
      const at = this.#order.indexOf(e.listing.id);
      return at === -1 ? this.#order.length : at;
    };
    return [...entries].sort((a, b) => rank(a) - rank(b));
  }

  /** Gives meter `id` another session store; one it does not hold is ignored. */
  replace(id: string, session: Session): void {
    this.#entries = this.#entries.map((e) => (e.listing.id === id ? { ...e, session } : e));
  }

  /** Drops a meter, with any recording parked for it; an expanded one goes back to the
   * grid of the rest. */
  remove(id: string): void {
    this.#entries = this.#entries.filter((e) => e.listing.id !== id);
    this.#order = this.#order.filter((o) => o !== id);
    this.#parkedPd.delete(id);
    if (this.expandedId === id) this.expandedId = null;
  }

  /** Routes a backend event to the store of meter `id`; one it does not hold is ignored. */
  handleEvent(id: string, event: BackendEvent): void {
    this.get(id)?.session.handleEvent(event);
  }

  /** Routes an event of meter `id`, outside the full view, to its tile. A disconnect the
   * user asked for takes the tile away; an unplug keeps it, to show the wait and the replug.
   * Its recording follows the events too, and ends with the session. */
  handleTileEvent(id: string, event: BackendEvent): void {
    this.handleEvent(id, event);
    this.#parkedPd.get(id)?.handleEvent(event);
    const recording = this.get(id)?.recording;
    if (event.type === "recording") recording?.handleEvent(event.data);
    if (event.type === "status" && event.data.state === "disconnected") {
      recording?.sessionEnded();
    }
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

  /** Keeps meter `id`'s PD capture going while the meter is away from the full view. */
  parkPd(id: string, capture: PdCapture): void {
    this.#parkedPd.set(id, capture);
  }

  /** Takes back meter `id`'s parked PD capture, if it has one. */
  unparkPd(id: string): PdCapture | undefined {
    const capture = this.#parkedPd.get(id);
    this.#parkedPd.delete(id);
    return capture;
  }

  /** Keeps meter `id`'s recording while the meter is away from the full view. */
  park(id: string, recording: RecordingSnapshot): void {
    this.get(id)?.recording.adopt(recording);
  }

  /** A recording is parked for meter `id`. */
  hasParked(id: string): boolean {
    return this.get(id)?.recording.phase === "recording";
  }

  /** The recording parked for meter `id`, once; undefined when none. */
  unpark(id: string): RecordingSnapshot | undefined {
    return this.get(id)?.recording.detach() ?? undefined;
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
