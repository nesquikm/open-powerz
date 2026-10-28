/** The grid of meter tiles (FR 1HHT03): each tile's title. The columns are CSS, in
 * MeterGrid.svelte. */

import type { DeviceListing } from "./backend";
import { deviceOptionLabel } from "./format";

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
