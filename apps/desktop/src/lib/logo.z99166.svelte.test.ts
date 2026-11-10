/** FR Z99166 AC-Z99166.1: over HID the Startup logo tab's Read and Show stored logo
 * are disabled with the HID note as their reason (`readDisabledReason`), and pressing
 * Read sends nothing and adds no error line, so the HID note shows once. Fake backends;
 * nothing reaches a meter. */

import { describe, expect, it, vi } from "vitest";
import type { LogoRead, MeterSettings, UiError } from "./backend";
import { StartupLogoState, type LogoDeps, type LogoMeter } from "./logo.svelte";
import { hidOnlyBasicText } from "./web/features";

const HID_NOTE = hidOnlyBasicText("The startup logo");

const SETTINGS: MeterSettings = {
  fields: [{ key: "startup_logo", label: "Startup logo", value: "off", tag: "confirmed" }],
};

const STORED: LogoRead = { status: "empty", width: null, height: null, mode: null, image: null };

/** What the HID transport answers a logo read with, were one sent. */
const HID_REFUSAL: UiError = { kind: "usage", message: HID_NOTE, hint: null };

function harness(meter: LogoMeter) {
  const read = vi.fn(async (): Promise<LogoRead> => {
    if (current.basicReadingsOnly === true) throw HID_REFUSAL;
    return STORED;
  });
  const deps = {
    choose: vi.fn(async () => null),
    preview: vi.fn(),
    read,
    upload: vi.fn(),
    remove: vi.fn(),
    writeSettings: vi.fn(async () => SETTINGS),
    checkSettings: vi.fn(),
    refreshSettings: vi.fn(async () => {}),
  } as unknown as LogoDeps & { read: typeof read };
  const current: LogoMeter = { ...meter };
  const state = new StartupLogoState(deps, () => current);
  return { deps, state, current };
}

type WithReadReason = StartupLogoState & { readDisabledReason: string | null };

describe("over HID Read and Show stored logo are disabled (AC-Z99166.1)", () => {
  it("z99166: over HID the read reason is the HID note", () => {
    const h = harness({ model: "POWER-Z KM003C", settings: SETTINGS, basicReadingsOnly: true });
    expect((h.state as WithReadReason).readDisabledReason).toBe(HID_NOTE);
  });

  it("z99166: over HID the read reason is the HID note before the model is read", () => {
    const h = harness({ model: null, settings: null, basicReadingsOnly: true });
    expect((h.state as WithReadReason).readDisabledReason).toBe(HID_NOTE);
  });

  it("z99166: over WebUSB there is no read reason", () => {
    const h = harness({ model: "POWER-Z KM003C", settings: SETTINGS, basicReadingsOnly: false });
    expect((h.state as WithReadReason).readDisabledReason).toBeNull();
  });

  it("z99166: over HID Read sends nothing and adds no error line", async () => {
    const h = harness({ model: "POWER-Z KM003C", settings: SETTINGS, basicReadingsOnly: true });
    await h.state.read();
    expect(h.deps.read).not.toHaveBeenCalled();
    expect(h.state.error).toBeNull();
    expect(h.state.area).toBeNull();
    expect(h.state.reading).toBe(false);
  });

  it("z99166: over HID Show stored logo sends nothing and adds no error line", async () => {
    const h = harness({ model: "POWER-Z KM003C", settings: SETTINGS, basicReadingsOnly: true });
    await h.state.read(true);
    expect(h.deps.read).not.toHaveBeenCalled();
    expect(h.state.error).toBeNull();
    expect(h.state.area).toBeNull();
  });

  it("z99166: the HID note shows once: as the reason, never also as the error", async () => {
    const h = harness({ model: "POWER-Z KM003C", settings: SETTINGS, basicReadingsOnly: true });
    await h.state.read();
    await h.state.read(true);
    const shown = [(h.state as WithReadReason).readDisabledReason, h.state.error?.message ?? null];
    expect(shown.filter((t) => t === HID_NOTE)).toHaveLength(1);
  });

  it("z99166: after a reconnect over WebUSB Read reads again", async () => {
    const h = harness({ model: "POWER-Z KM003C", settings: SETTINGS, basicReadingsOnly: true });
    h.current.basicReadingsOnly = false;
    h.state.resetSession();
    expect((h.state as WithReadReason).readDisabledReason).toBeNull();
    await h.state.read();
    expect(h.deps.read).toHaveBeenCalledTimes(1);
    expect(h.state.area).toEqual(STORED);
  });
});
