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
 * suffix; `reportedSerial` (what the connected meter reported) wins over the listing's.
 * With serial numbers hidden, any meter is its model and position, "KM003C 2" (AC-AXHK2Q.4). */
export function tileTitle(
  listing: DeviceListing,
  reportedSerial: string | null,
  masked: { hidden: boolean; position: number },
): { name: string; suffix: string } {
  if (masked.hidden) return { name: `${listing.model} ${masked.position}`, suffix: "" };
  const name = listing.demo_scenario !== null ? deviceOptionLabel(listing, false) : listing.model;
  return { name, suffix: serialSuffix(reportedSerial ?? listing.serial) };
}

/** What a control bar is called: it names the meter its controls act on (AC-4PJRVM.1). */
export function controlsLabel(title: { name: string; suffix: string }): string {
  return `Controls for ${title.name}${title.suffix ? ` ${title.suffix}` : ""}`;
}
