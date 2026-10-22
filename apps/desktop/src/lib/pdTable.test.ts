/**
 * FR TGE83C (AC .2, .4, .5, .6, .7): the PD packet table's view logic, free of the DOM:
 * the virtual list's row window, following new rows, the hex view with the selected
 * field's bytes highlighted, and the cell texts and screen reader label of a row.
 */

import { describe, expect, it } from "vitest";
import type { PdRow } from "./backend";
import {
  directionLabel,
  FollowState,
  hexCells,
  rowLabel,
  timeLabel,
  visibleRange,
} from "./pdTable";

const caps: PdRow = {
  index: 41,
  elapsed_ms: 300,
  kind: "message",
  sop: "SOP",
  name: "Source_Capabilities",
  message_id: 0,
  direction: "source_to_sink",
  objects: 4,
  spec_revision: "3.x",
  vbus_v: 5.01,
  ibus_a: 0.02,
  summary: "Source_Capabilities: Fixed 5.00 V 3.00 A",
};

describe("visibleRange: the virtual list renders only the rows in view (AC-TGE83C.6)", () => {
  it("covers the viewport plus the overscan, clamped to the row count", () => {
    // 24 px rows, a 240 px viewport scrolled to row 100.
    const r = visibleRange({
      scrollTop: 2400,
      viewportPx: 240,
      rowPx: 24,
      count: 100_000,
      overscan: 5,
    });
    expect(r.start).toBe(95);
    expect(r.end).toBe(115);
  });

  it("starts at 0 at the top and ends at the count at the bottom", () => {
    expect(
      visibleRange({ scrollTop: 0, viewportPx: 240, rowPx: 24, count: 100_000, overscan: 5 }).start,
    ).toBe(0);
    const bottom = visibleRange({
      scrollTop: 100_000 * 24 - 240,
      viewportPx: 240,
      rowPx: 24,
      count: 100_000,
      overscan: 5,
    });
    expect(bottom.end).toBe(100_000);
    expect(bottom.end - bottom.start).toBeLessThanOrEqual(10 + 10);
  });

  it("is empty with no rows and never runs past a short list", () => {
    expect(
      visibleRange({ scrollTop: 0, viewportPx: 240, rowPx: 24, count: 0, overscan: 5 }),
    ).toEqual({
      start: 0,
      end: 0,
    });
    expect(
      visibleRange({ scrollTop: 0, viewportPx: 240, rowPx: 24, count: 3, overscan: 5 }),
    ).toEqual({
      start: 0,
      end: 3,
    });
  });
});

describe("FollowState: the table follows new rows until the user scrolls up (AC-TGE83C.5)", () => {
  it("follows from the start", () => {
    expect(new FollowState().following).toBe(true);
  });

  it("keeps following while the view sits at the bottom", () => {
    const f = new FollowState();
    f.scrolled({ scrollTop: 760, clientHeight: 240, scrollHeight: 1000 });
    expect(f.following).toBe(true);
  });

  it("scrolling up pauses following", () => {
    const f = new FollowState();
    f.scrolled({ scrollTop: 760, clientHeight: 240, scrollHeight: 1000 });
    f.scrolled({ scrollTop: 400, clientHeight: 240, scrollHeight: 1000 });
    expect(f.following).toBe(false);
  });

  it("new rows below a paused view do not resume it", () => {
    const f = new FollowState();
    f.scrolled({ scrollTop: 400, clientHeight: 240, scrollHeight: 1000 });
    // More rows arrived: the content grew, the view stayed put.
    f.scrolled({ scrollTop: 400, clientHeight: 240, scrollHeight: 5000 });
    expect(f.following).toBe(false);
  });

  it("Follow resumes it", () => {
    const f = new FollowState();
    f.scrolled({ scrollTop: 400, clientHeight: 240, scrollHeight: 1000 });
    f.resume();
    expect(f.following).toBe(true);
  });
});

describe("hexCells: raw bytes in hex with the selected field highlighted (AC-TGE83C.4)", () => {
  const raw = [0xa1, 0x11, 0x2c, 0x91, 0x01, 0x0a];

  it("shows every byte as two upper-case hex digits", () => {
    expect(hexCells(raw, null).map((c) => c.text)).toEqual(["A1", "11", "2C", "91", "01", "0A"]);
    expect(hexCells(raw, null).every((c) => !c.highlighted)).toBe(true);
  });

  it("highlights the bytes of the selected field's [start, end) range", () => {
    const cells = hexCells(raw, [2, 6]);
    expect(cells.map((c) => c.highlighted)).toEqual([false, false, true, true, true, true]);
    expect(hexCells(raw, [0, 1]).map((c) => c.highlighted)).toEqual([
      true,
      false,
      false,
      false,
      false,
      false,
    ]);
  });

  it("highlights nothing for an empty range", () => {
    expect(hexCells(raw, [3, 3]).some((c) => c.highlighted)).toBe(false);
  });
});

describe("row texts (AC-TGE83C.2, AC-TGE83C.7)", () => {
  it("names the direction in words", () => {
    expect(directionLabel("source_to_sink")).toMatch(/source.*sink/i);
    expect(directionLabel("sink_to_source")).toMatch(/sink.*source/i);
    expect(directionLabel("cable")).toMatch(/cable/i);
    expect(directionLabel(null)).toBe("");
  });

  it("shows the time since capture start in seconds with ms precision", () => {
    expect(timeLabel(300)).toBe("0.300 s");
    expect(timeLabel(61_234)).toBe("61.234 s");
  });

  it("gives a row a screen reader label with its number, time, message and summary", () => {
    const label = rowLabel(caps);
    expect(label).toContain("42"); // rows are numbered from 1 for people
    expect(label).toContain("0.300 s");
    expect(label).toContain("Source_Capabilities");
    expect(label).toMatch(/source.*sink/i);
    expect(label).toContain("Fixed 5.00 V 3.00 A");
  });

  it("labels an event row as an event", () => {
    const label = rowLabel({
      ...caps,
      kind: "event",
      name: "Hard Reset",
      sop: null,
      message_id: null,
      direction: null,
      objects: null,
      spec_revision: null,
      summary: "Hard Reset",
    });
    expect(label).toMatch(/event/i);
    expect(label).toContain("Hard Reset");
  });
});
