/**
 * FR TGE83D: the chart's per-draw range lookups stay cheap on a long capture.
 *
 * - `markersInRange` binary-searches a store whose session times never go back, and
 *   classifies only the rows inside the range; a store whose times do go back (a run
 *   that started on an earlier clock) still answers correctly by a full scan.
 * - `PdStatusBuffer.bounds` gives the `[lo, hi)` index range of a time range over the
 *   buffer's own columns, so a draw copies nothing.
 */

import { describe, expect, it, vi } from "vitest";
import type { PdRow, PdStatusSample } from "./backend";
import { PdStore } from "./pdStore";
import { PdStatusBuffer, markersInRange } from "./pdTimeline";

function row(index: number, sessionMs: number): PdRow {
  return {
    index,
    elapsed_ms: sessionMs,
    session_ms: sessionMs,
    kind: index % 10 === 0 ? "event" : "message",
    sop: "SOP",
    name: index % 10 === 0 ? "Attach" : "Request",
    message_id: 0,
    direction: "source_to_sink",
    objects: 0,
    spec_revision: "3.x",
    vbus_v: 5,
    ibus_a: 0.5,
    summary: "",
  };
}

/** `n` rows, two per session millisecond (each time repeats), from `t0`. */
function bigStore(n: number, t0 = 1_000): PdStore {
  const store = new PdStore();
  const rows: PdRow[] = [];
  for (let i = 0; i < n; i++) rows.push(row(i, t0 + Math.floor(i / 2)));
  store.append(rows);
  return store;
}

/** The brute-force answer: every row with `t0 <= ms <= t1`, in capture order. */
function brute(store: PdStore, t0: number, t1: number): number[] {
  const out: number[] = [];
  for (let i = 0; i < store.length; i++) {
    const ms = store.sessionMs(i);
    if (ms >= t0 && ms <= t1) out.push(store.indexAt(i));
  }
  return out;
}

describe("markersInRange on a large store", () => {
  const store = bigStore(100_000);

  it.each([
    [1_000, 1_000],
    [999, 1_000],
    [1_010, 1_020],
    [1_010.5, 1_020.5],
    [50_999, 51_000],
    [50_999, 60_000],
    [0, 999],
    [51_000, 99_000],
    [0, Infinity],
    [2_000, 1_000],
  ])("matches a full scan over %d..=%d, edges included", (t0, t1) => {
    expect(markersInRange(store, t0, t1).map((m) => m.index)).toEqual(brute(store, t0, t1));
  });

  it("looks at the rows inside the range only", () => {
    const s = bigStore(100_000);
    const names = vi.spyOn(s, "name");
    const times = vi.spyOn(s, "sessionMs");
    const got = markersInRange(s, 20_000, 20_009);
    expect(got).toHaveLength(20);
    expect(names).toHaveBeenCalledTimes(20);
    // Two binary searches plus one read per row inside.
    expect(times.mock.calls.length).toBeLessThan(100);
  });

  it("still answers when session time goes back between runs", () => {
    const s = new PdStore();
    s.append([row(0, 5_000), row(1, 5_010), row(2, 1_000), row(3, 1_005), row(4, 6_000)]);
    expect(markersInRange(s, 1_000, 5_005).map((m) => m.index)).toEqual([0, 2, 3]);
    expect(markersInRange(s, 5_010, 6_000).map((m) => m.index)).toEqual([1, 4]);
  });

  it("a clear makes a store searchable again", () => {
    const s = new PdStore();
    s.append([row(0, 5_000), row(1, 1_000)]);
    s.clear();
    s.append([row(2, 2_000), row(3, 3_000)]);
    const names = vi.spyOn(s, "name");
    expect(markersInRange(s, 2_500, 3_000).map((m) => m.index)).toEqual([3]);
    expect(names).toHaveBeenCalledTimes(1);
  });
});

function status(ms: number, v: number): PdStatusSample {
  return { session_ms: ms, vbus_v: v, ibus_a: -v, cc1_v: v / 10, cc2_v: 0 };
}

describe("PdStatusBuffer.bounds", () => {
  it("gives the [lo, hi) index range of a time range over the buffer's own columns", () => {
    const buf = new PdStatusBuffer();
    buf.append([status(100, 1), status(105, 2), status(110, 3)]);
    buf.append([status(110, 4), status(115, 5), status(120, 6)]);
    expect(buf.bounds(105, 115)).toEqual([1, 5]);
    expect(buf.bounds(106, 109)).toEqual([2, 2]);
    expect(buf.bounds(0, 99)).toEqual([0, 0]);
    expect(buf.bounds(121, 200)).toEqual([6, 6]);
    expect(buf.bounds(0, Infinity)).toEqual([0, 6]);
    const [lo, hi] = buf.bounds(110, 110);
    const cols = buf.columns;
    expect(cols.t.slice(lo, hi)).toEqual([110, 110]);
    expect(cols.vbus.slice(lo, hi)).toEqual([3, 4]);
    expect(cols.ibus.slice(lo, hi)).toEqual([-3, -4]);
    expect(cols.cc1.slice(lo, hi)).toEqual([0.3, 0.4]);
    expect(cols.cc2.slice(lo, hi)).toEqual([0, 0]);
  });

  it("hands out its columns without copying them", () => {
    const buf = new PdStatusBuffer();
    buf.append([status(1, 1)]);
    expect(buf.columns.t).toBe(buf.columns.t);
    buf.append([status(2, 2)]);
    expect(buf.columns.t).toHaveLength(2);
  });

  it("is empty after a clear", () => {
    const buf = new PdStatusBuffer();
    buf.append([status(1, 1), status(2, 2)]);
    buf.clear();
    expect(buf.bounds(0, 10)).toEqual([0, 0]);
    expect(buf.columns.t).toHaveLength(0);
  });
});
