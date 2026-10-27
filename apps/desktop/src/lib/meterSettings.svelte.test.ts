/** Meter settings panel logic (FR 1HHT01, AC .2 and .4): tagged read-only rows,
 * a read on connect and on demand, cleared on disconnect. */

import { describe, expect, it, vi } from "vitest";
import type { MeterSettings, UiError } from "./backend";
import { MeterSettingsPanel, meterSettingRows, tagText } from "./meterSettings.svelte";

const SETTINGS: MeterSettings = {
  fields: [
    { key: "language", label: "Language", value: "English", tag: "inferred" },
    { key: "brightness", label: "Brightness", value: "50%", tag: "confirmed" },
    { key: "offset_cal", label: "Calibration offset", value: "80", tag: "inferred" },
  ],
};

const refused: UiError = { kind: "usage", message: "No meter is connected.", hint: null };

describe("meterSettingRows (AC-1HHT01.2)", () => {
  it("keeps every field in order with its value and a tag text", () => {
    expect(meterSettingRows(SETTINGS)).toEqual([
      {
        key: "language",
        label: "Language",
        value: "English",
        tag: "inferred",
        tagText: "inferred",
      },
      {
        key: "brightness",
        label: "Brightness",
        value: "50%",
        tag: "confirmed",
        tagText: "confirmed",
      },
      {
        key: "offset_cal",
        label: "Calibration offset",
        value: "80",
        tag: "inferred",
        tagText: "inferred",
      },
    ]);
  });

  it("words both tags", () => {
    expect(tagText("inferred")).toBe("inferred");
    expect(tagText("confirmed")).toBe("confirmed");
  });
});

describe("MeterSettingsPanel (AC-1HHT01.4)", () => {
  it("starts empty and reads nothing by itself", () => {
    const read = vi.fn(async () => SETTINGS);
    const p = new MeterSettingsPanel(read);
    expect(p.settings).toBeNull();
    expect(p.error).toBeNull();
    expect(p.loading).toBe(false);
    expect(read).not.toHaveBeenCalled();
  });

  it("reads the settings when the meter connects", async () => {
    const read = vi.fn(async () => SETTINGS);
    const p = new MeterSettingsPanel(read);
    await p.connected();
    expect(read).toHaveBeenCalledTimes(1);
    expect(p.settings).toEqual(SETTINGS);
    expect(p.loading).toBe(false);
  });

  it("reads again on refresh", async () => {
    const read = vi.fn(async () => SETTINGS);
    const p = new MeterSettingsPanel(read);
    await p.connected();
    await p.refresh();
    expect(read).toHaveBeenCalledTimes(2);
    expect(p.settings).toEqual(SETTINGS);
  });

  it("is loading while a read is in flight", async () => {
    let resolve: (s: MeterSettings) => void = () => {};
    const read = vi.fn(() => new Promise<MeterSettings>((r) => (resolve = r)));
    const p = new MeterSettingsPanel(read);
    const done = p.refresh();
    expect(p.loading).toBe(true);
    resolve(SETTINGS);
    await done;
    expect(p.loading).toBe(false);
  });

  it("shows a failed read (a CRC mismatch, say) as an error and no stale values", async () => {
    const crc: UiError = {
      kind: "protocol",
      message: "SETTINGS block A: CRC mismatch",
      hint: null,
    };
    const read = vi.fn<() => Promise<MeterSettings>>();
    read.mockResolvedValueOnce(SETTINGS).mockRejectedValueOnce(crc);
    const p = new MeterSettingsPanel(read);
    await p.connected();
    await p.refresh();
    expect(p.settings).toBeNull();
    expect(p.error).toEqual(crc);
  });

  it("a successful refresh clears an earlier error", async () => {
    const read = vi.fn<() => Promise<MeterSettings>>();
    read.mockRejectedValueOnce(refused).mockResolvedValueOnce(SETTINGS);
    const p = new MeterSettingsPanel(read);
    await p.refresh();
    expect(p.error).toEqual(refused);
    await p.refresh();
    expect(p.error).toBeNull();
    expect(p.settings).toEqual(SETTINGS);
  });

  it("forgets the settings on disconnect", async () => {
    const read = vi.fn(async () => SETTINGS);
    const p = new MeterSettingsPanel(read);
    await p.connected();
    p.disconnected();
    expect(p.settings).toBeNull();
    expect(p.error).toBeNull();
  });

  it("drops a read that finishes after the meter went away", async () => {
    let resolve: (s: MeterSettings) => void = () => {};
    const read = vi.fn(() => new Promise<MeterSettings>((r) => (resolve = r)));
    const p = new MeterSettingsPanel(read);
    const done = p.connected();
    p.disconnected();
    resolve(SETTINGS);
    await done;
    expect(p.settings).toBeNull();
    expect(p.loading).toBe(false);
  });
});
