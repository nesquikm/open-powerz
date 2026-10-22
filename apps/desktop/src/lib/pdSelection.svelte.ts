/**
 * The PD selection shared by the packet table and the power chart (FR TGE83D, AC .2,
 * .3).
 */

/** Where a selection was made. */
export type PdSelectionOrigin = "chart" | "table";

/** What the selection follows: the capture's count of empties (a clear, a new
 * session), after which row indices start over. */
export interface PdSelectionSource {
  readonly emptied: number;
}

export class PdSelection {
  #index: number | null = $state(null);
  #origin: PdSelectionOrigin | null = $state(null);
  /** The source's `emptied` count when the selection was made. */
  #emptiedAt = $state(0);
  readonly #source: PdSelectionSource | null;
  /** Counts the selections. */
  seq = $state(0);

  /** A selection that drops itself whenever `source` is emptied, whether or not a
   * view that shows it is mounted. */
  constructor(source: PdSelectionSource | null = null) {
    this.#source = source;
  }

  /** The selection is from before the source's last empty. */
  #stale(): boolean {
    return this.#source !== null && this.#source.emptied !== this.#emptiedAt;
  }

  /** The selected record index, or null. */
  get index(): number | null {
    return this.#stale() ? null : this.#index;
  }

  /** Where it was selected, or null. */
  get origin(): PdSelectionOrigin | null {
    return this.#stale() ? null : this.#origin;
  }

  /** Selects record `index`, made from `origin`. */
  select(index: number, origin: PdSelectionOrigin): void {
    this.#index = index;
    this.#origin = origin;
    this.#emptiedAt = this.#source?.emptied ?? 0;
    this.seq++;
  }

  /** Drops the selection. */
  clear(): void {
    this.#index = null;
    this.#origin = null;
  }
}
