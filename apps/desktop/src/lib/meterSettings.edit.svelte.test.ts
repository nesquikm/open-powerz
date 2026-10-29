/** Meter settings panel edit logic (FR JKWT3W, AC .3, .4 and .5): only the
 * writable fields get edit controls, each with its valid values; the panel shows a
 * summary of the change before Apply sends it, sends only the changed fields, shows
 * the block as read back, and on a failure shows the error once without trying
 * again. */

import { describe, expect, it, vi } from "vitest";
import type { MeterSettings, SettingChanges, UiError } from "./backend";
import {
  EDITABLE_SETTINGS,
  MeterSettingsPanel,
  SAVED_NOTICE,
  changeSummary,
  settingCodes,
} from "./meterSettings.svelte";

/** The demo meter's rows (captured FW 2.0.6 block). */
const SETTINGS: MeterSettings = {
  fields: [
    { key: "language", label: "Language", value: "English", tag: "confirmed" },
    { key: "brightness", label: "Brightness", value: "50%", tag: "confirmed" },
    { key: "dim_timeout", label: "Dim screen after", value: "off", tag: "confirmed" },
    { key: "startup_logo", label: "Startup logo", value: "off", tag: "confirmed" },
    { key: "theme", label: "Theme", value: "blue", tag: "confirmed" },
    { key: "auto_rotate", label: "Auto-rotate", value: "on", tag: "confirmed" },
    { key: "rotation", label: "Rotation", value: "90°", tag: "inferred" },
    { key: "sample_rate", label: "Sample rate", value: "50 SPS", tag: "confirmed" },
    { key: "voltage_cal", label: "Voltage calibration (raw)", value: "606", tag: "inferred" },
    { key: "offset_cal", label: "Offset calibration (raw)", value: "80", tag: "inferred" },
  ],
};

function withValue(s: MeterSettings, key: string, value: string): MeterSettings {
  return { fields: s.fields.map((f) => (f.key === key ? { ...f, value } : f)) };
}

const AFTER = withValue(withValue(SETTINGS, "brightness", "80%"), "theme", "pink");

const mismatch: UiError = {
  kind: "protocol",
  message: "After the write the meter shows brightness 50%, expected 80%.",
  hint: null,
};

function harness(fail?: UiError, result: MeterSettings = AFTER) {
  const read = vi.fn(async () => SETTINGS);
  const write = vi.fn<(changes: SettingChanges) => Promise<MeterSettings>>(async () => {
    if (fail) throw fail;
    return result;
  });
  const panel = new MeterSettingsPanel(read, write);
  return { read, write, panel };
}

async function editing(fail?: UiError) {
  const h = harness(fail);
  await h.panel.connected();
  h.panel.startEdit();
  return h;
}

describe("EDITABLE_SETTINGS (AC-JKWT3W.3)", () => {
  it("lists only the writable fields, each with its valid values", () => {
    expect(EDITABLE_SETTINGS.map((s) => s.key)).toEqual([
      "language",
      "brightness",
      "dim_timeout",
      "theme",
      "rotation",
      "sample_rate",
    ]);
    const options = (key: string) =>
      EDITABLE_SETTINGS.find((s) => s.key === key)?.options?.map((o) => [o.code, o.label]);
    expect(options("language")).toEqual([
      [0, "Chinese"],
      [1, "English"],
    ]);
    expect(options("dim_timeout")).toEqual([
      [0, "off"],
      [1, "15 s"],
      [2, "30 s"],
      [3, "60 s"],
    ]);
    expect(options("theme")).toEqual([
      [0, "blue"],
      [1, "green"],
      [2, "pink"],
      [3, "orange"],
      [4, "yellow"],
      [5, "dark green"],
      [6, "cyan"],
    ]);
    expect(options("rotation")).toEqual([
      [0, "0°"],
      [1, "90°"],
      [2, "180°"],
      [3, "270°"],
    ]);
    expect(options("sample_rate")).toEqual([
      [0, "2 SPS"],
      [1, "10 SPS"],
      [2, "50 SPS"],
      [3, "1000 SPS"],
    ]);
    const brightness = EDITABLE_SETTINGS.find((s) => s.key === "brightness");
    expect([brightness?.min, brightness?.max]).toEqual([1, 100]);
  });

  it("never offers calibration, auto-rotate or the startup logo", () => {
    const keys: string[] = EDITABLE_SETTINGS.map((s) => s.key);
    for (const k of ["voltage_cal", "current_cal", "offset_cal", "auto_rotate", "startup_logo"]) {
      expect(keys).not.toContain(k);
    }
  });
});

describe("settingCodes / changeSummary (AC-JKWT3W.5)", () => {
  it("reads the current code of every writable field from the rows", () => {
    expect(settingCodes(SETTINGS)).toEqual({
      language: 1,
      brightness: 50,
      dim_timeout: 0,
      theme: 0,
      rotation: 1,
      sample_rate: 2,
    });
  });

  it("words each change as label, old value and new value", () => {
    expect(changeSummary(SETTINGS, { brightness: 80, theme: 2 })).toEqual([
      "Brightness: 50% → 80%",
      "Theme: blue → pink",
    ]);
    expect(changeSummary(SETTINGS, { rotation: 3, language: 0 })).toEqual([
      "Language: English → Chinese",
      "Rotation: 90° → 270°",
    ]);
    expect(changeSummary(SETTINGS, {})).toEqual([]);
  });
});

describe("MeterSettingsPanel edit (AC-JKWT3W.4, .5)", () => {
  it("starts read-only; Edit fills the draft with the values just read", async () => {
    const { panel } = harness();
    await panel.connected();
    expect(panel.editing).toBe(false);
    panel.startEdit();
    expect(panel.editing).toBe(true);
    expect(panel.draft).toEqual(settingCodes(SETTINGS));
    expect(panel.changes).toEqual({});
    expect(panel.summary).toEqual([]);
  });

  it("cannot edit before the settings are read", () => {
    const { panel } = harness();
    panel.startEdit();
    expect(panel.editing).toBe(false);
  });

  it("tracks only the changed fields and summarises them before Apply", async () => {
    const { panel, write } = await editing();
    expect(panel.setDraft("brightness", 80)).toBe(true);
    expect(panel.setDraft("theme", 2)).toBe(true);
    expect(panel.setDraft("language", 1)).toBe(true); // unchanged value
    expect(panel.changes).toEqual({ brightness: 80, theme: 2 });
    expect(panel.summary).toEqual(["Brightness: 50% → 80%", "Theme: blue → pink"]);
    expect(write).not.toHaveBeenCalled();
    // Setting a field back drops it from the change.
    panel.setDraft("theme", 0);
    expect(panel.changes).toEqual({ brightness: 80 });
  });

  it("refuses values outside a field's range", async () => {
    const { panel } = await editing();
    for (const [key, bad] of [
      ["brightness", 0],
      ["brightness", 101],
      ["brightness", 12.5],
      ["theme", 7],
      ["language", -1],
      ["rotation", 4],
      ["sample_rate", 4],
      ["dim_timeout", 4],
    ] as const) {
      expect(panel.setDraft(key, bad)).toBe(false);
    }
    expect(panel.changes).toEqual({});
  });

  it("Apply sends only the changes once and shows the block as read back", async () => {
    const { panel, write } = await editing();
    panel.setDraft("brightness", 80);
    panel.setDraft("theme", 2);
    await panel.apply();
    expect(write).toHaveBeenCalledTimes(1);
    expect(write).toHaveBeenCalledWith({ brightness: 80, theme: 2 });
    expect(panel.settings).toEqual(AFTER);
    expect(panel.editing).toBe(false);
    expect(panel.error).toBeNull();
    expect(panel.applying).toBe(false);
  });

  it("a successful Apply says the meter's screen shows it after its menu is opened", async () => {
    const { panel } = await editing();
    expect(panel.notice).toBeNull();
    panel.setDraft("brightness", 80);
    await panel.apply();
    expect(SAVED_NOTICE).toBe(
      "The meter has saved the change; its screen shows it after you open that setting in the meter's menu.",
    );
    expect(panel.notice).toBe(SAVED_NOTICE);
  });

  it("the saved notice goes away on the next edit and on disconnect", async () => {
    const { panel } = await editing();
    panel.setDraft("brightness", 80);
    await panel.apply();
    panel.startEdit();
    expect(panel.notice).toBeNull();
    panel.setDraft("brightness", 70);
    await panel.apply();
    panel.disconnected();
    expect(panel.notice).toBeNull();
  });

  it("a failed write shows no saved notice", async () => {
    const { panel } = await editing(mismatch);
    panel.setDraft("brightness", 80);
    await panel.apply();
    expect(panel.notice).toBeNull();
  });

  it("a refresh while Apply is in flight is ignored, so the write's answer still lands", async () => {
    const { panel, read } = await editing();
    panel.setDraft("brightness", 80);
    const done = panel.apply();
    await panel.refresh();
    await done;
    expect(read).toHaveBeenCalledTimes(1);
    expect(panel.settings).toEqual(AFTER);
    expect(panel.applying).toBe(false);
    expect(panel.editing).toBe(false);
  });

  it("Apply with nothing changed sends nothing", async () => {
    const { panel, write } = await editing();
    await panel.apply();
    expect(write).not.toHaveBeenCalled();
  });

  it("a failed write shows the error once, keeps the draft and is not retried", async () => {
    const { panel, write } = await editing(mismatch);
    panel.setDraft("brightness", 80);
    await panel.apply();
    expect(write).toHaveBeenCalledTimes(1);
    expect(panel.error).toEqual(mismatch);
    expect(panel.editing).toBe(true);
    expect(panel.changes).toEqual({ brightness: 80 });
    expect(panel.applying).toBe(false);
  });

  it("Cancel drops the draft without writing", async () => {
    const { panel, write } = await editing();
    panel.setDraft("brightness", 80);
    panel.cancelEdit();
    expect(panel.editing).toBe(false);
    expect(panel.changes).toEqual({});
    expect(write).not.toHaveBeenCalled();
    expect(panel.settings).toEqual(SETTINGS);
  });

  it("a disconnect ends the edit", async () => {
    const { panel } = await editing();
    panel.setDraft("brightness", 80);
    panel.disconnected();
    expect(panel.editing).toBe(false);
    expect(panel.changes).toEqual({});
  });
});
