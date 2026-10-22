/**
 * The PD analyzer's record store (FR TGE83C): rows arrive in `pd_rows` batches and live
 * in columns (typed arrays, with repeated strings interned), so 100,000 records stay
 * cheap to keep and to filter. Filters pick store positions and never drop data.
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

const KIND_EVENT = 1;

export class PdStore {
  length = 0;
  #capacity: number;
  #index = new Float64Array(0);
  #elapsed = new Float64Array(0);
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

  constructor(capacity = 1024) {
    this.#capacity = Math.max(1, capacity);
    this.#alloc(this.#capacity);
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

  /** Appends a batch, in order. */
  append(rows: readonly PdRow[]): void {
    if (this.length + rows.length > this.#capacity) {
      this.#alloc(Math.max(this.length + rows.length, this.#capacity * 2));
    }
    for (const r of rows) {
      const i = this.length++;
      this.#index[i] = r.index;
      this.#elapsed[i] = r.elapsed_ms;
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
    }
  }

  /** The row at store position `i`. */
  row(i: number): PdRow {
    const id = this.#messageId[i] ?? -1;
    const objects = this.#objects[i] ?? -1;
    return {
      index: this.#index[i] ?? 0,
      elapsed_ms: this.#elapsed[i] ?? 0,
      kind: this.#kind[i] === KIND_EVENT ? "event" : "message",
      sop: this.#strings.value(this.#sop[i] ?? NONE),
      name: this.#strings.value(this.#name[i] ?? NONE) ?? "",
      message_id: id < 0 ? null : id,
      direction: this.#strings.value(this.#direction[i] ?? NONE) as PdDirection | null,
      objects: objects < 0 ? null : objects,
      spec_revision: this.#strings.value(this.#revision[i] ?? NONE),
      vbus_v: this.#vbus[i] ?? 0,
      ibus_a: this.#ibus[i] ?? 0,
      summary: this.#summary[i] ?? "",
    };
  }

  /** Drops every row; the store starts over. */
  clear(): void {
    this.length = 0;
    this.#summary = [];
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

  /** The message types seen, in first-seen order (for the filter menu). */
  messageTypes(): string[] {
    return this.#distinct(this.#name);
  }

  /** The SOPs seen, in first-seen order (for the filter menu). */
  sops(): string[] {
    return this.#distinct(this.#sop);
  }

  #distinct(col: Uint32Array): string[] {
    const seen = new Set<number>();
    const out: string[] = [];
    for (let i = 0; i < this.length; i++) {
      if (this.#kind[i] === KIND_EVENT) continue;
      const id = col[i] as number;
      if (id === NONE || seen.has(id)) continue;
      seen.add(id);
      const v = this.#strings.value(id);
      if (v !== null) out.push(v);
    }
    return out;
  }
}
