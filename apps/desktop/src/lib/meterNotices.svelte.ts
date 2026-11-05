/** One notice store per meter (FR AXHK2K), read by its tile and the full view, so a notice
 * stays with its meter wherever that meter is shown. */

import { SvelteMap } from "svelte/reactivity";

export interface MeterNotice {
  /** `info` notices of the full view's meter leave by themselves, errors stay. */
  tone: "info" | "error";
  /** Bold first line, when there is one. */
  message?: string | null;
  text: string;
  /** The file it speaks of, for the reveal and copy actions. */
  path: string | null;
}

const NONE: readonly MeterNotice[] = [];

export class MeterNotices {
  #byMeter = new SvelteMap<string, readonly MeterNotice[]>();
  /** The notice each meter's recording shows now: a recording has one at a time. */
  #ofRecording = new SvelteMap<string, MeterNotice>();

  /** Meter `id`'s notices, oldest first. */
  of(id: string): readonly MeterNotice[] {
    return this.#byMeter.get(id) ?? NONE;
  }

  /** Adds a notice to meter `id`. */
  raise(id: string, notice: MeterNotice): void {
    this.#byMeter.set(id, [...this.of(id), notice]);
  }

  /** Takes one notice of meter `id` away. */
  dismiss(id: string, notice: MeterNotice): void {
    this.#byMeter.set(
      id,
      this.of(id).filter((n) => n !== notice),
    );
  }

  /** What meter `id`'s recording says now, in the place of what it said before; null when
   * it has nothing to say. */
  setRecording(id: string, notice: MeterNotice | null): void {
    const held = this.#ofRecording.get(id);
    const list = this.of(id);
    const at = held === undefined ? -1 : list.indexOf(held);
    if (notice === null) this.#ofRecording.delete(id);
    else this.#ofRecording.set(id, notice);
    if (at === -1) {
      if (notice !== null) this.raise(id, notice);
      return;
    }
    this.#byMeter.set(
      id,
      list.flatMap((n, i) => (i !== at ? [n] : notice === null ? [] : [notice])),
    );
  }

  /** Hands the notices of a meter that left for good to the meter that took its place. */
  move(from: string, to: string): void {
    const moved = this.of(from);
    this.#ofRecording.delete(from);
    this.#byMeter.delete(from);
    if (moved.length > 0) this.#byMeter.set(to, [...this.of(to), ...moved]);
  }
}
