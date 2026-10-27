/** The grid of meter tiles (FR 1HHT03): its columns and each tile's title. */

import type { DeviceListing } from "./backend";
import { deviceOptionLabel } from "./format";
import { LAYOUT_BREAKPOINT_PX } from "./layout";

/** Columns of the tile grid for a window `width` CSS px wide holding `count` meters: two
 * side by side on a wide window, one (stacked) on a narrow one or for a single meter. */
export function gridColumns(width: number, count: number): number {
  if (count < 2 || width < LAYOUT_BREAKPOINT_PX) return 1;
  return 2;
}

/** The last four characters of `serial` after an ellipsis; a short serial whole, none empty. */
export function serialSuffix(serial: string | null): string {
  if (!serial) return "";
  return serial.length > 4 ? `…${serial.slice(-4)}` : serial;
}

/** A tile's title: a real meter by model, a demo meter by its scenario, each with the serial
 * suffix; `reportedSerial` (what the connected meter reported) wins over the listing's. */
export function tileTitle(
  listing: DeviceListing,
  reportedSerial: string | null,
): { name: string; suffix: string } {
  const name = listing.demo_scenario !== null ? deviceOptionLabel(listing) : listing.model;
  return { name, suffix: serialSuffix(reportedSerial ?? listing.serial) };
}
