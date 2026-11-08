/**
 * FR AYR4W6: in the web build the connect actions are items of the device menu. Each
 * action's `run` opens the browser's device picker; `chooseDeviceMenu` calls it
 * synchronously in the select's change event, so the choice still counts as the user
 * gesture WebUSB / WebHID need (AC-AYR4W6.1, AC-AYR4W6.3).
 */

import { cancelWait, requestHidMeter, requestMeter } from "./backend";
import { pickMeter } from "./pickMeter";
import type { Session } from "./session.svelte";

export interface DeviceMenuAction {
  /** The option's value; never a device id. */
  value: string;
  label: string;
  run: () => Promise<void>;
}

const CONNECT_VALUE = "action:connect";
const CONNECT_HID_VALUE = "action:connect-hid";

/** The menu's action items, in menu order (shown after the devices); the first is
 * always the WebUSB connect item, which the first-visit hint reuses. */
export function deviceMenuActions(session: Session, offerHid: boolean): DeviceMenuAction[] {
  const actions: DeviceMenuAction[] = [
    {
      value: CONNECT_VALUE,
      label: "Connect a meter…",
      run: () => pickMeter(session, requestMeter, cancelWait),
    },
  ];
  if (offerHid) {
    actions.push({
      value: CONNECT_HID_VALUE,
      label: "Connect over HID…",
      run: () => pickMeter(session, requestHidMeter, cancelWait),
    });
  }
  return actions;
}

/** Handles a choice in the device menu. An action item runs at once and the select goes
 * back to the picked device; a device is selected. Returns whether a pending wait must
 * be cancelled (see `Session.select`). */
export function chooseDeviceMenu(
  session: Session,
  target: { value: string },
  actions: readonly { value: string; run: () => unknown }[],
): boolean {
  const action = actions.find((a) => a.value === target.value);
  if (action === undefined) return session.select(target.value);
  void action.run();
  target.value = session.selectedId ?? "";
  return false;
}
