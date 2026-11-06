/**
 * FR 70X5YY: a startup logo write holds every session action, not only the controls that
 * send to the meter. Pure conditions the app and its controls derive from the hold.
 */

import type { LogoPhase } from "./logo.svelte";
import { recordButton, type RecordingPhase } from "./recording.svelte";
import type { ShortcutAction } from "./shortcuts";

/** The tooltip of every control the write holds. */
export const LOGO_HOLD_TITLE = "Unavailable: a startup logo write is running";

/** Only the write itself, from the first chunk to the end of the read-back, holds. */
export function logoHolds(phase: LogoPhase): boolean {
  return phase === "writing";
}

/** The session actions' own conditions, before the hold. */
export interface SessionConditions {
  canRecord: boolean;
  canExport: boolean;
  canAddMeter: boolean;
}

export interface SessionActions extends SessionConditions {
  canOpenRecording: boolean;
  canPause: boolean;
  canRecorderOptions: boolean;
}

/** The session actions with the hold folded in. */
export function sessionActions(own: SessionConditions, held: boolean): SessionActions {
  return {
    canRecord: own.canRecord && !held,
    canExport: own.canExport && !held,
    canAddMeter: own.canAddMeter && !held,
    canOpenRecording: !held,
    canPause: !held,
    canRecorderOptions: !held,
  };
}

/** Whether a Record (or Stop) button takes no press: the hold covers every phase. */
export function recordDisabled(phase: RecordingPhase, canRecord: boolean, held: boolean): boolean {
  return held || recordButton(phase, canRecord).disabled;
}

/** A held control's tooltip names the write; otherwise the control keeps its own. */
export function holdTitle(held: boolean, own: string | undefined): string | undefined {
  return held ? LOGO_HOLD_TITLE : own;
}

/** Tabs a firmware update holds; a logo write holds these and the Firmware tab too. */
const FIRMWARE_HELD_TABS = new Set(["pd", "trigger", "records"]);
const LOGO_HELD_TABS = new Set(["pd", "trigger", "records", "firmware"]);

/** Whether tab `id` is disabled by a running firmware update or logo write. */
export function tabHeld(id: string, state: { firmwareBusy: boolean; logoHeld: boolean }): boolean {
  return (
    (state.firmwareBusy && FIRMWARE_HELD_TABS.has(id)) || (state.logoHeld && LOGO_HELD_TABS.has(id))
  );
}

/** Shortcuts that start a session action or send to the meter. */
const HELD_SHORTCUTS = new Set<ShortcutAction["kind"]>(["record", "connect", "rate"]);

/** Whether a shortcut does nothing while the write holds the session. */
export function shortcutHeld(action: ShortcutAction, held: boolean): boolean {
  return held && HELD_SHORTCUTS.has(action.kind);
}
