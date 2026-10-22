/**
 * The PD packet table's view logic, free of the DOM (FR TGE83C): the virtual list's
 * row window, following new rows, the hex view and the texts of a row.
 */

import type { PdDirection, PdRow } from "./backend";

/** The rows a virtual list renders: `[start, end)`. */
export interface RowRange {
  start: number;
  end: number;
}

/** The rows in view plus `overscan` on each side, clamped to `count`. */
export function visibleRange(v: {
  scrollTop: number;
  viewportPx: number;
  rowPx: number;
  count: number;
  overscan: number;
}): RowRange {
  if (v.count <= 0 || v.rowPx <= 0) return { start: 0, end: 0 };
  const first = Math.floor(Math.max(0, v.scrollTop) / v.rowPx);
  const inView = Math.ceil(Math.max(0, v.viewportPx) / v.rowPx);
  const start = Math.min(v.count, Math.max(0, first - v.overscan));
  const end = Math.min(v.count, first + inView + v.overscan);
  return { start, end: Math.max(start, end) };
}

/** How close to the bottom still counts as at the bottom, px. */
const BOTTOM_SLACK_PX = 4;

/** Whether the table follows new rows: it does until the user scrolls up. */
export class FollowState {
  following = true;
  #lastTop: number | null = null;

  /** The view scrolled (or its content grew). */
  scrolled(v: { scrollTop: number; clientHeight: number; scrollHeight: number }): void {
    const atBottom = v.scrollTop + v.clientHeight >= v.scrollHeight - BOTTOM_SLACK_PX;
    if (atBottom) this.following = true;
    else if (this.#lastTop === null || v.scrollTop < this.#lastTop || !this.following) {
      this.following = false;
    }
    this.#lastTop = v.scrollTop;
  }

  /** The Follow button. */
  resume(): void {
    this.following = true;
  }
}

/** One byte of the hex view. */
export interface HexCell {
  text: string;
  highlighted: boolean;
}

/** `raw` as upper-case hex, with the bytes of `range` (`[start, end)`) highlighted. */
export function hexCells(
  raw: readonly number[],
  range: readonly [number, number] | null,
): HexCell[] {
  return raw.map((b, i) => ({
    text: (b & 0xff).toString(16).toUpperCase().padStart(2, "0"),
    highlighted: range !== null && i >= range[0] && i < range[1],
  }));
}

/** The direction in words. */
export function directionLabel(d: PdDirection | null): string {
  switch (d) {
    case "source_to_sink":
      return "Source → Sink";
    case "sink_to_source":
      return "Sink → Source";
    case "cable":
      return "Cable";
    default:
      return "";
  }
}

/** Time since capture start, in seconds with ms precision. */
export function timeLabel(ms: number): string {
  return `${(ms / 1000).toFixed(3)} s`;
}

/** The screen reader label of a row; rows are numbered from 1 for people. */
export function rowLabel(r: PdRow): string {
  const head = `Row ${r.index + 1}, ${timeLabel(r.elapsed_ms)}`;
  if (r.kind === "event") return `${head}, event ${r.name}: ${r.summary}`;
  const dir = directionLabel(r.direction).replace("→", "to");
  return `${head}, ${r.sop ?? ""} ${r.name}, ${dir}: ${r.summary}`;
}
