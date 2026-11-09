/**
 * FR QFTCX0, AC-QFTCX0.2: the "Connect a meter…" click. `request` is called at once,
 * before any await, so the browser still sees the click as the user gesture its device
 * picker needs. The granted meter joins the device list (once) and is selected, which,
 * as when it is chosen by hand, ends any wait for another meter (FR XPT7PZ). A cancelled
 * picker changes nothing, and a failure is reported rather than thrown.
 */

import { toUiError, type DeviceListing } from "./backend";
import type { Session } from "./session.svelte";

export async function pickMeter(
  session: Session,
  request: () => Promise<DeviceListing | null>,
  cancelWait?: () => Promise<void>,
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
    if (!session.devices.some((d) => d.id === meter.id)) {
      session.setDevices([...session.devices, meter]);
    }
    // FR XPT7PZ, AC-XPT7PZ.1: the granted meter is the one the device menu picks.
    // AC-XPT7PZ.5: like choosing it by hand, selecting it ends a wait for another meter.
    if (session.select(meter.id) && cancelWait !== undefined) await cancelWait();
  } catch (err) {
    session.reportError(toUiError(err));
  }
}
