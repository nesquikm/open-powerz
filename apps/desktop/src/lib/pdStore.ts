/**
 * The PD analyzer's record store (FR TGE83C): rows arrive in `pd_rows` batches and live
 * in columns (typed arrays, with repeated strings interned), so 100,000 records stay
 * cheap to keep and to filter. Filters pick store positions and never drop data; only
 * the row limit does (FR 4PJRVK, AC .3): past it, the oldest rows go and are counted.
 */

import type { PdDirection, PdRow } from "./backend";

/** What the packet table shows. */
export interface PdFilter {
  /** Show GoodCRC messages. */
  showGoodCrc: boolean;
  /** Only these message types (null: all). */
  types: ReadonlySet<string> | null;
  /** Only messages on this SOP (null: all); events pass. */
  sop: string | null;
}

/** GoodCRC hidden, every type, every SOP. */
export const DEFAULT_PD_FILTER: Readonly<PdFilter> = Object.freeze({
  showGoodCrc: false,
  types: null,
  sop: null,
});

/** Rows a store keeps by default; the backend keeps as many records (`MAX_RECORDS`), so
 * every row in the table still has its details. */
export const PD_MAX_ROWS = 100_000;

/** The notice of a capture that dropped `dropped` rows, or null when it dropped none. */
export function pdDroppedText(dropped: number): string | null {
  if (dropped === 0) return null;
  const n = dropped.toLocaleString("en-US");
  return `Dropped the oldest ${n} ${dropped === 1 ? "record" : "records"}: a capture keeps the last ${PD_MAX_ROWS.toLocaleString("en-US")}.`;
}

/** Code of a missing value in the interned columns. */
const NONE = 0;

/** Interns strings as small numbers (0 stands for null). */
class Interner {
  #ids = new Map<string, number>();
  #values: (string | null)[] = [null];

  id(value: string | null): number {
    if (value === null) return NONE;
    let id = this.#ids.get(value);
    if (id === undefined) {
      id = this.#values.length;
      this.#values.push(value);
      this.#ids.set(value, id);
    }
    return id;
  }

  /** The id of `value` if it was ever interned. */
  find(value: string): number | undefined {
    return this.#ids.get(value);
  }

  value(id: number): string | null {
    return this.#values[id] ?? null;
  }
}

/** Distinct values in first-seen order; {@link values} changes identity only when a new
 * value arrives. */
class FirstSeen {
  #seen = new Set<string>();
  values: readonly string[] = [];

  add(value: string | null): void {
    if (value === null || this.#seen.has(value)) return;
    this.#seen.add(value);
    this.values = [...this.values, value];
  }
}

const KIND_EVENT = 1;

export class PdStore {
  length = 0;
  #capacity: number;
  #maxRows: number;
  #dropped = 0;
  #index = new Float64Array(0);
  #elapsed = new Float64Array(0);
  /** Session time (FR TGE83D); NaN when the row had none. */
  #session = new Float64Array(0);
  #vbus = new Float64Array(0);
  #ibus = new Float64Array(0);
  /** Message ID, objects: -1 for null. */
  #messageId = new Int8Array(0);
  #objects = new Int8Array(0);
  #kind = new Uint8Array(0);
  #name = new Uint32Array(0);
  #sop = new Uint32Array(0);
  #direction = new Uint32Array(0);
  #revision = new Uint32Array(0);
  #summary: string[] = [];
  #strings = new Interner();
  /** Session times never go back (and none is missing) since the last clear. */
  #sessionSorted = true;
  #types = new FirstSeen();
  #sops = new FirstSeen();

  constructor(capacity = 1024, { maxRows = PD_MAX_ROWS }: { maxRows?: number } = {}) {
    this.#maxRows = Math.max(1, maxRows);
    this.#capacity = Math.min(Math.max(1, capacity), this.#maxRows);
    this.#alloc(this.#capacity);
  }

  /** Rows dropped since the last clear to stay within the row limit. */
  get dropped(): number {
    return this.#dropped;
  }

  #alloc(capacity: number): void {
    const grow = <T extends Float64Array | Int8Array | Uint8Array | Uint32Array>(
      old: T,
      make: (n: number) => T,
    ): T => {
      const next = make(capacity);
      next.set(old.subarray(0, this.length) as never);
      return next;
    };
    this.#index = grow(this.#index, (n) => new Float64Array(n));
    this.#elapsed = grow(this.#elapsed, (n) => new Float64Array(n));
    this.#session = grow(this.#session, (n) => new Float64Array(n));
    this.#vbus = grow(this.#vbus, (n) => new Float64Array(n));
    this.#ibus = grow(this.#ibus, (n) => new Float64Array(n));
    this.#messageId = grow(this.#messageId, (n) => new Int8Array(n));
    this.#objects = grow(this.#objects, (n) => new Int8Array(n));
    this.#kind = grow(this.#kind, (n) => new Uint8Array(n));
    this.#name = grow(this.#name, (n) => new Uint32Array(n));
    this.#sop = grow(this.#sop, (n) => new Uint32Array(n));
    this.#direction = grow(this.#direction, (n) => new Uint32Array(n));
    this.#revision = grow(this.#revision, (n) => new Uint32Array(n));
    this.#capacity = capacity;
  }

  /** Drops the oldest `n` rows; the rest move to the front of the columns. */
  #dropOldest(n: number): void {
    for (const col of [
      this.#index,
      this.#elapsed,
      this.#session,
      this.#vbus,
      this.#ibus,
      this.#messageId,
      this.#objects,
      this.#kind,
      this.#name,
      this.#sop,
      this.#direction,
      this.#revision,
    ]) {
      col.copyWithin(0, n, this.length);
    }
    this.#summary.splice(0, n);
    this.length -= n;
    this.#dropped += n;
  }

  /** Counts `n` rows that never got here because the backend dropped them past its
   * limit (an opened capture), so {@link dropped} stays right. */
  addDropped(n: number): void {
    this.#dropped += Math.max(0, n);
  }

  /** Appends a batch, in order, dropping the oldest rows past the row limit. */
  append(rows: readonly PdRow[]): void {
    const skipped = Math.max(0, rows.length - this.#maxRows);
    const kept = skipped > 0 ? rows.slice(skipped) : rows;
    this.#dropped += skipped;
    const over = this.length + kept.length - this.#maxRows;
    if (over > 0) this.#dropOldest(over);
    const needed = this.length + kept.length;
    if (needed > this.#capacity) {
      this.#alloc(Math.min(this.#maxRows, Math.max(needed, this.#capacity * 2)));
    }
    for (const r of kept) {
      const i = this.length++;
      this.#index[i] = r.index;
      this.#elapsed[i] = r.elapsed_ms;
      const session = r.session_ms ?? NaN;
      const prev = i === 0 ? -Infinity : this.#session[i - 1]!;
      // NaN compares false, so a missing time also ends the sorted order.
      if (!(session >= prev)) this.#sessionSorted = false;
      this.#session[i] = session;
      this.#vbus[i] = r.vbus_v;
      this.#ibus[i] = r.ibus_a;
      this.#messageId[i] = r.message_id ?? -1;
      this.#objects[i] = r.objects ?? -1;
      this.#kind[i] = r.kind === "event" ? KIND_EVENT : 0;
      this.#name[i] = this.#strings.id(r.name);
      this.#sop[i] = this.#strings.id(r.sop);
      this.#direction[i] = this.#strings.id(r.direction);
      this.#revision[i] = this.#strings.id(r.spec_revision);
      this.#summary[i] = r.summary;
      if (r.kind !== "event") {
        this.#types.add(r.name);
        this.#sops.add(r.sop);
      }
    }
  }

  /** The row at store position `i`. */
  row(i: number): PdRow {
    const id = this.#messageId[i] ?? -1;
    const objects = this.#objects[i] ?? -1;
    const session = this.sessionMs(i);
    return {
      index: this.#index[i] ?? 0,
      elapsed_ms: this.#elapsed[i] ?? 0,
      ...(Number.isNaN(session) ? {} : { session_ms: session }),
      kind: this.isEvent(i) ? "event" : "message",
      sop: this.#strings.value(this.#sop[i] ?? NONE),
      name: this.name(i),
      message_id: id < 0 ? null : id,
      direction: this.#strings.value(this.#direction[i] ?? NONE) as PdDirection | null,
      objects: objects < 0 ? null : objects,
      spec_revision: this.#strings.value(this.#revision[i] ?? NONE),
      vbus_v: this.#vbus[i] ?? 0,
      ibus_a: this.#ibus[i] ?? 0,
      summary: this.#summary[i] ?? "",
    };
  }

  /** The session time of store position `i` (NaN when unknown). */
  sessionMs(i: number): number {
    return this.#session[i] ?? NaN;
  }

  /** Whether session times never go back (and none is missing), so a time range can
   * be binary-searched; a run on an earlier clock can break it until the next clear. */
  get sessionSorted(): boolean {
    return this.#sessionSorted;
  }

  /** Whether store position `i` holds an event. */
  isEvent(i: number): boolean {
    return this.#kind[i] === KIND_EVENT;
  }

  /** The message or event name at store position `i`. */
  name(i: number): string {
    return this.#strings.value(this.#name[i] ?? NONE) ?? "";
  }

  /** The record index at store position `i`. */
  indexAt(i: number): number {
    return this.#index[i] ?? -1;
  }

  /** The store position of record `index`, or -1 (FR TGE83D, AC .2). */
  positionOf(index: number): number {
    // Record indices ascend in capture order.
    let lo = 0;
    let hi = this.length - 1;
    while (lo <= hi) {
      const mid = (lo + hi) >> 1;
      const at = this.#index[mid] ?? -1;
      if (at === index) return mid;
      if (at < index) lo = mid + 1;
      else hi = mid - 1;
    }
    return -1;
  }

  /** Drops every row; the store starts over. */
  clear(): void {
    this.length = 0;
    this.#summary = [];
    this.#sessionSorted = true;
    this.#dropped = 0;
    this.#types = new FirstSeen();
    this.#sops = new FirstSeen();
  }

  /** The store positions `filter` lets through, in capture order. */
  filter(filter: Readonly<PdFilter>): Uint32Array {
    const out = new Uint32Array(this.length);
    const goodCrc = filter.showGoodCrc ? undefined : this.#strings.find("GoodCRC");
    const sop = filter.sop === null ? undefined : (this.#strings.find(filter.sop) ?? -1);
    let types: Set<number> | null = null;
    if (filter.types !== null) {
      types = new Set();
      for (const t of filter.types) {
        const id = this.#strings.find(t);
        if (id !== undefined) types.add(id);
      }
    }
    let n = 0;
    for (let i = 0; i < this.length; i++) {
      const name = this.#name[i] as number;
      if (types !== null && !types.has(name)) continue;
      if (this.#kind[i] !== KIND_EVENT) {
        if (goodCrc !== undefined && name === goodCrc) continue;
        if (sop !== undefined && this.#sop[i] !== sop) continue;
      }
      out[n++] = i;
    }
    return out.subarray(0, n);
  }

  /** The message types seen since the last clear, in first-seen order (for the filter
   * menu); a new list only when a new type arrived. */
  messageTypes(): readonly string[] {
    return this.#types.values;
  }

  /** The SOPs seen since the last clear, in first-seen order (for the filter menu); a
   * new list only when a new SOP arrived. */
  sops(): readonly string[] {
    return this.#sops.values;
  }
}
