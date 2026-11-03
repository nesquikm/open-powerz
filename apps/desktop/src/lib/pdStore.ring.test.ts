/**
 * FR AXHK2W, AC .4: dropping the oldest rows from `PdStore` shifts no columns. Once the
 * store is at its row limit, an append moves a constant number of elements, whatever
 * the limit: the test counts every element that typed-array and array methods move
 * while appends run, and checks the rows still read right after the columns wrap.
 */

import { afterEach, describe, expect, it } from "vitest";
import type { PdRow } from "./backend";
import { PdStore } from "./pdStore";

function msg(index: number, name = "Accept", sop = "SOP"): PdRow {
  return {
    index,
    elapsed_ms: index * 10,
    session_ms: 1_000 + index * 10,
    kind: "message",
    sop,
    name,
    message_id: index % 8,
    direction: "source_to_sink",
    objects: 0,
    spec_revision: "3.x",
    vbus_v: 5 + index / 1000,
    ibus_a: 0.5,
    summary: `${name} #${index}`,
  };
}

type Restore = () => void;

/** Counts the elements moved by the methods that shift or copy whole column stretches,
 * on typed arrays and plain arrays, until restored. */
function countMoves(): { moved: () => number; restore: Restore } {
  let moved = 0;
  const restores: Restore[] = [];
  const typedProto = Object.getPrototypeOf(Int8Array.prototype) as Record<string, unknown>;
  const arrayProto = Array.prototype as unknown as Record<string, unknown>;
  const patch = (
    proto: Record<string, unknown>,
    name: string,
    count: (self: ArrayLike<unknown>, args: unknown[]) => number,
  ): void => {
    const original = proto[name] as (...args: unknown[]) => unknown;
    proto[name] = function (this: ArrayLike<unknown>, ...args: unknown[]) {
      moved += count(this, args);
      return original.apply(this, args);
    };
    restores.push(() => {
      proto[name] = original;
    });
  };
  const span = (len: number, start: unknown, end: unknown): number => {
    const norm = (v: unknown, dflt: number): number => {
      const n = v === undefined ? dflt : Number(v);
      return n < 0 ? Math.max(0, len + n) : Math.min(len, n);
    };
    return Math.max(0, norm(end, len) - norm(start, 0));
  };
  for (const proto of [typedProto, arrayProto]) {
    patch(proto, "copyWithin", (self, [, start, end]) => span(self.length, start, end));
  }
  patch(typedProto, "set", (_self, [src]) => (src as ArrayLike<unknown>).length);
  patch(arrayProto, "splice", (self, [start, del, ...items]) => {
    const s = span(self.length, 0, start);
    const d = Math.min(Number(del ?? self.length - s), self.length - s);
    // The elements after the removed stretch shift when the counts differ.
    return items.length === d ? 0 : Math.max(0, self.length - s - d);
  });
  patch(arrayProto, "shift", (self) => Math.max(0, self.length - 1));
  patch(arrayProto, "unshift", (self) => self.length);
  return {
    moved: () => moved,
    restore: () => {
      for (const r of restores.reverse()) r();
    },
  };
}

let active: Restore | null = null;
afterEach(() => {
  active?.();
  active = null;
});

/** Elements moved by `appends` single-row appends to a store full at `maxRows`. */
function movedAtCap(maxRows: number, appends: number): number {
  const store = new PdStore(maxRows, { maxRows });
  store.append(Array.from({ length: maxRows }, (_, i) => msg(i)));
  const counter = countMoves();
  active = counter.restore;
  for (let k = 0; k < appends; k++) store.append([msg(maxRows + k)]);
  counter.restore();
  active = null;
  expect(store.length).toBe(maxRows);
  expect(store.dropped).toBe(appends);
  return counter.moved();
}

describe("appends at the row limit (AC-AXHK2W.4)", () => {
  it("AC-AXHK2W.4: an append at the cap moves no column stretch", () => {
    // A column shift moves every kept row of every column: about 12 × 9,999 here.
    const moved = movedAtCap(10_000, 200);
    expect(moved).toBeLessThanOrEqual(200 * 16);
  });

  it("AC-AXHK2W.4: an append at the cap costs the same at any limit (constant time)", () => {
    const small = movedAtCap(1_000, 100);
    const large = movedAtCap(100_000, 100);
    expect(large).toBe(small);
    expect(large).toBeLessThanOrEqual(100 * 16);
  });

  it("AC-AXHK2W.4: a batch at the cap moves elements in proportion to the batch, not the store", () => {
    const store = new PdStore(50_000, { maxRows: 50_000 });
    store.append(Array.from({ length: 50_000 }, (_, i) => msg(i)));
    const counter = countMoves();
    active = counter.restore;
    store.append(Array.from({ length: 64 }, (_, k) => msg(50_000 + k)));
    counter.restore();
    active = null;
    expect(store.dropped).toBe(64);
    expect(counter.moved()).toBeLessThanOrEqual(64 * 16);
  });

  it("AC-AXHK2W.4: rows read right after the columns wrap", () => {
    const store = new PdStore(4, { maxRows: 8 });
    for (let i = 0; i < 8; i++) store.append([msg(i, i % 2 ? "GoodCRC" : "Accept")]);
    // Several wraps, in odd-sized batches.
    store.append([msg(8), msg(9, "Request", "SOP'"), msg(10)]);
    store.append([msg(11, "GoodCRC"), msg(12), msg(13, "Request"), msg(14), msg(15)]);
    store.append([msg(16)]);
    expect(store.length).toBe(8);
    expect(store.dropped).toBe(9);
    const indices = Array.from({ length: store.length }, (_, i) => store.indexAt(i));
    expect(indices).toEqual([9, 10, 11, 12, 13, 14, 15, 16]);
    expect(store.row(0)).toEqual(msg(9, "Request", "SOP'"));
    expect(store.row(7)).toEqual(msg(16));
    expect(store.name(2)).toBe("GoodCRC");
    expect(store.sessionMs(4)).toBe(1_000 + 13 * 10);
    expect(store.positionOf(13)).toBe(4);
    expect(store.positionOf(8)).toBe(-1);
    expect(store.positionOf(16)).toBe(7);
    expect(store.sessionSorted).toBe(true);
    // GoodCRC hidden: positions 2 (index 11) goes.
    expect([...store.filter({ showGoodCrc: false, types: null, sop: null })]).toEqual([
      0, 1, 3, 4, 5, 6, 7,
    ]);
    expect([...store.filter({ showGoodCrc: true, types: null, sop: "SOP'" })]).toEqual([0]);
    expect([
      ...store.filter({ showGoodCrc: true, types: new Set(["Request"]), sop: null }),
    ]).toEqual([0, 4]);
  });

  it("AC-AXHK2W.4: a clear after a wrap starts the store over", () => {
    const store = new PdStore(4, { maxRows: 4 });
    store.append(Array.from({ length: 11 }, (_, i) => msg(i)));
    store.clear();
    store.append([msg(100), msg(101)]);
    expect(store.length).toBe(2);
    expect(store.row(0)).toEqual(msg(100));
    expect(store.positionOf(101)).toBe(1);
  });
});
