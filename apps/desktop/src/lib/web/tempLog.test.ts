/**
 * FR JFRPME: the web history's temperature log, as the desktop's `TempLog`
 * (`src-tauri/src/store.rs`): entries of (first sample index, °C, last session time it
 * applies to), one per temperature change, extended while the same reading repeats
 * before it expires.
 *
 * - AC-JFRPME.1 / .2: the lookup gives a sample the latest reading not older than the
 *   age limit, and nothing before the first reading or once it expired.
 * - AC-JFRPME.3: one entry per change, not per sample or snapshot; clear drops them.
 */

import { describe, expect, it } from "vitest";
import { TempLog } from "./tempLog";

/** The temperatures at `points` (`[index, session ms]`, increasing), with one cursor. */
function lookups(log: TempLog, points: [number, number][]): (number | null)[] {
  const cursor = { i: 0 };
  return points.map(([index, t]) => log.at(index, t, cursor));
}

describe("lookup (AC-JFRPME.1, AC-JFRPME.2)", () => {
  it("an empty log has no temperature", () => {
    const log = new TempLog();
    expect(log.entries).toHaveLength(0);
    expect(lookups(log, [[0, 0]])).toEqual([null]);
  });

  it("gives a reading from its first sample until its expiry, inclusive", () => {
    const log = new TempLog();
    log.record(3, 31.5, 100, 2_100);
    expect(
      lookups(log, [
        [0, 0],
        [2, 50],
        [3, 100],
        [7, 1_500],
        [8, 2_100],
        [9, 2_101],
      ]),
    ).toEqual([null, null, 31.5, 31.5, 31.5, null]);
  });

  it("switches to the next reading at its first sample", () => {
    const log = new TempLog();
    log.record(0, 30, 0, 2_000);
    log.record(4, 31.25, 400, 2_400);
    expect(
      lookups(log, [
        [0, 0],
        [3, 300],
        [4, 400],
        [6, 2_300],
        [7, 2_401],
      ]),
    ).toEqual([30, 30, 31.25, 31.25, null]);
  });

  it("a cursor walks past several entries in one step", () => {
    const log = new TempLog();
    log.record(0, 30, 0, 2_000);
    log.record(2, 31, 200, 2_200);
    log.record(4, 32, 400, 2_400);
    log.record(6, 33, 600, 2_600);
    expect(
      lookups(log, [
        [0, 0],
        [7, 700],
      ]),
    ).toEqual([30, 33]);
  });
});

describe("one entry per change (AC-JFRPME.3)", () => {
  it("the same reading recorded for many samples stays one entry", () => {
    const log = new TempLog();
    for (let i = 0; i < 50; i++) log.record(i, 30, 0, 2_000);
    expect(log.entries).toEqual([[0, 30, 2_000]]);
  });

  it("the same temperature read again before it expired extends the entry", () => {
    const log = new TempLog();
    log.record(0, 30, 0, 2_000);
    log.record(5, 30, 500, 2_500);
    log.record(10, 30, 1_000, 3_000);
    expect(log.entries).toEqual([[0, 30, 3_000]]);
    expect(
      lookups(log, [
        [9, 2_900],
        [12, 3_000],
        [13, 3_001],
      ]),
    ).toEqual([30, 30, null]);
  });

  it("an older reading never shortens the entry", () => {
    const log = new TempLog();
    log.record(0, 30, 1_000, 3_000);
    log.record(3, 30, 500, 2_500);
    expect(log.entries).toEqual([[0, 30, 3_000]]);
  });

  it("the same temperature read after it expired starts a new entry", () => {
    const log = new TempLog();
    log.record(0, 30, 0, 2_000);
    log.record(10, 30, 2_500, 4_500);
    expect(log.entries).toEqual([
      [0, 30, 2_000],
      [10, 30, 4_500],
    ]);
    expect(
      lookups(log, [
        [5, 1_900],
        [9, 2_200],
        [10, 2_500],
      ]),
    ).toEqual([30, null, 30]);
  });

  it("a different temperature starts a new entry", () => {
    const log = new TempLog();
    log.record(0, 30, 0, 2_000);
    log.record(1, 30.5, 100, 2_100);
    log.record(2, 30, 200, 2_200);
    expect(log.entries).toEqual([
      [0, 30, 2_000],
      [1, 30.5, 2_100],
      [2, 30, 2_200],
    ]);
  });

  it("clear drops every entry", () => {
    const log = new TempLog();
    log.record(0, 30, 0, 2_000);
    log.record(4, 31, 400, 2_400);
    log.clear();
    expect(log.entries).toHaveLength(0);
    expect(lookups(log, [[0, 0]])).toEqual([null]);
    log.record(0, 29.5, 3_000, 5_000);
    expect(log.entries).toEqual([[0, 29.5, 5_000]]);
  });
});
