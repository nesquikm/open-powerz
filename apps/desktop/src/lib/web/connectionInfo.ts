/**
 * FR 60QB2S: the text of the (i) popover beside the web device menu, which explains how
 * the browser edition connects to the meter.
 */

import type { ConnectedInfo, DeviceListing } from "../backend";
import type { ConnectionState } from "../format";

/** Short plain sentences, one per connection, shown in the popover in this order. */
export const CONNECTION_INFO_LINES: readonly string[] = [
  "WebUSB gives the full feature set: live readings, PD capture, rates, records and the logo.",
  "HID (Connect over HID…) gives basic readings only, and it works even when another program holds the meter.",
  "Web Serial is used only for settings changes and the protocol trigger. Chrome asks once for the meter's serial port.",
  "Chrome or Edge can connect to the meter. Safari and Firefox show the demo only.",
];

/** The parts of the live session the current-connection line reads. */
export interface ConnectionSessionView {
  devices: readonly DeviceListing[];
  selectedId: string | null;
  state: ConnectionState;
  details?: Partial<ConnectedInfo> | null;
}

/** The popover's line naming this session's connection (AC-60QB2S.5): WebUSB, HID, the
 * demo meter, or none yet. */
export function currentConnectionLine(session: ConnectionSessionView): string {
  const connected = session.state === "streaming" || session.state === "rate_locked";
  const device = session.devices.find((d) => d.id === session.selectedId);
  if (!connected || device === undefined) return "Current connection: none yet.";
  if (device.demo_scenario !== null) return "Current connection: the demo meter.";
  if (session.details?.basic_readings_only === true) return "Current connection: HID.";
  return "Current connection: WebUSB.";
}
