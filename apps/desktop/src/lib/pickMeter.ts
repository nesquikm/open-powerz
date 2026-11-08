/**
 * FR QFTCX0, AC-QFTCX0.2: the "Connect a meter…" click. `request` is called at once,
 * before any await, so the browser still sees the click as the user gesture its device
 * picker needs. The granted meter joins the device list (once); a cancelled picker
 * changes nothing, and a failure is reported rather than thrown.
 */

import { toUiError, type DeviceListing } from "./backend";
import type { Session } from "./session.svelte";

export async function pickMeter(
  session: Session,
  request: () => Promise<DeviceListing | null>,
): Promise<void> {
  let pending: Promise<DeviceListing | null>;
  try {
    pending = request();
  } catch (err) {
    session.reportError(toUiError(err));
    return;
  }
  try {
    const meter = await pending;
    if (meter === null) return;
    if (session.devices.some((d) => d.id === meter.id)) return;
    session.setDevices([...session.devices, meter]);
  } catch (err) {
    session.reportError(toUiError(err));
  }
}
