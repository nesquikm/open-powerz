/**
 * FR 70X5YY: a startup logo write holds every session action, not only the controls that
 * send to the meter. The pure conditions the app and its controls derive from the hold.
 */

import { describe, expect, it } from "vitest";
import type { LogoPhase } from "./logo.svelte";
import {
  LOGO_HOLD_TITLE,
  holdTitle,
  logoHolds,
  recordDisabled,
  sessionActions,
  shortcutHeld,
  tabHeld,
} from "./logoHold";
import type { ShortcutAction } from "./shortcuts";

const ALL_PHASES: LogoPhase[] = [
  "idle",
  "previewed",
  "confirming",
  "writing",
  "saving",
  "done",
  "failed",
];

const OPEN = { canRecord: true, canExport: true, canAddMeter: true };

describe("the hold covers every session action (AC-70X5YY.1)", () => {
  it("AC-70X5YY.1: only the writing phase, from the first chunk to the end of the read-back, holds", () => {
    expect(logoHolds("writing")).toBe(true);
  });

  it("AC-70X5YY.1: while held, Add meter, Open recording, Record, Pause, Options and Export are all off", () => {
    expect(sessionActions(OPEN, true)).toEqual({
      canRecord: false,
      canExport: false,
      canAddMeter: false,
      canOpenRecording: false,
      canPause: false,
      canRecorderOptions: false,
    });
  });

  it("AC-70X5YY.1: the hold disables Record in every phase, Stop of a running recording too", () => {
    for (const phase of ["idle", "recording", "starting", "stopping"] as const) {
      expect(recordDisabled(phase, true, true), phase).toBe(true);
    }
  });

  it("AC-70X5YY.1: each held control's tooltip says a startup logo write is running", () => {
    expect(LOGO_HOLD_TITLE).toMatch(/startup logo write is running/i);
    expect(holdTitle(true, "Export the session so far to CSV")).toBe(LOGO_HOLD_TITLE);
    expect(holdTitle(true, undefined)).toBe(LOGO_HOLD_TITLE);
    expect(holdTitle(true, "Record to CSV (Space)")).toMatch(/startup logo write is running/i);
  });
});

describe("shortcuts during the write (AC-70X5YY.2)", () => {
  it("AC-70X5YY.2: Space (record) does nothing while held", () => {
    expect(shortcutHeld({ kind: "record" }, true)).toBe(true);
  });

  it("AC-70X5YY.2: the record shortcut works again once the write ends", () => {
    expect(shortcutHeld({ kind: "record" }, false)).toBe(false);
  });

  it("AC-70X5YY.2: theme, help and the chart's own view keys are never held", () => {
    const free: ShortcutAction[] = [
      { kind: "theme" },
      { kind: "help" },
      { kind: "live" },
      { kind: "home" },
      { kind: "fit" },
      { kind: "zoom", direction: "in" },
      { kind: "shuttle", key: "K" },
      { kind: "step", direction: 1, coarse: false },
    ];
    for (const action of free) expect(shortcutHeld(action, true), action.kind).toBe(false);
  });
});

describe("tabs during the write (AC-70X5YY.3)", () => {
  const held = { firmwareBusy: false, logoHeld: true };

  it("AC-70X5YY.3: PD analyzer, Trigger, Records and Firmware are disabled", () => {
    for (const id of ["pd", "trigger", "records", "firmware"]) {
      expect(tabHeld(id, held), id).toBe(true);
    }
  });

  it("AC-70X5YY.3: Meter and Startup logo stay usable", () => {
    expect(tabHeld("meter", held)).toBe(false);
    expect(tabHeld("logo", held)).toBe(false);
  });

  it("AC-70X5YY.3: a firmware update still holds pd, trigger and records, not its own tab", () => {
    const flashing = { firmwareBusy: true, logoHeld: false };
    for (const id of ["pd", "trigger", "records"]) expect(tabHeld(id, flashing), id).toBe(true);
    expect(tabHeld("firmware", flashing)).toBe(false);
    expect(tabHeld("meter", flashing)).toBe(false);
  });
});

describe("what stays usable (AC-70X5YY.4)", () => {
  it("AC-70X5YY.4: the hold leaves the theme, help and chart view shortcuts alone", () => {
    expect(shortcutHeld({ kind: "theme" }, true)).toBe(false);
    expect(shortcutHeld({ kind: "zoom", direction: "out" }, true)).toBe(false);
  });

  it("AC-70X5YY.4: a running recording can be stopped once the write ends", () => {
    expect(recordDisabled("recording", true, false)).toBe(false);
  });
});

describe("everything comes back when the write ends (AC-70X5YY.5)", () => {
  it("AC-70X5YY.5: no phase but writing holds: verified, failed and the menu-save wait are free", () => {
    for (const phase of ALL_PHASES.filter((p) => p !== "writing")) {
      expect(logoHolds(phase), phase).toBe(false);
    }
  });

  it("AC-70X5YY.5: unheld, the actions follow their own conditions", () => {
    expect(sessionActions(OPEN, false)).toEqual({
      canRecord: true,
      canExport: true,
      canAddMeter: true,
      canOpenRecording: true,
      canPause: true,
      canRecorderOptions: true,
    });
    expect(
      sessionActions({ canRecord: false, canExport: false, canAddMeter: false }, false),
    ).toMatchObject({ canRecord: false, canExport: false, canAddMeter: false });
  });

  it("AC-70X5YY.5: unheld, each control keeps its own tooltip", () => {
    expect(holdTitle(false, "Export the session so far to CSV")).toBe(
      "Export the session so far to CSV",
    );
    expect(holdTitle(false, undefined)).toBeUndefined();
  });

  it("AC-70X5YY.5: unheld, the tabs follow only the firmware update", () => {
    for (const id of ["pd", "trigger", "records", "firmware", "meter", "logo"]) {
      expect(tabHeld(id, { firmwareBusy: false, logoHeld: false }), id).toBe(false);
    }
  });

  it("AC-70X5YY.5: unheld, Record follows the recording phase as before", () => {
    expect(recordDisabled("idle", true, false)).toBe(false);
    expect(recordDisabled("idle", false, false)).toBe(true);
    expect(recordDisabled("starting", true, false)).toBe(true);
  });
});
