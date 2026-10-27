/** The demo backend's meter settings (FR 1HHT01, AC .4): the captured FW 2.0.6
 * block's fields, all tagged, only while a meter is connected. */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { MockBackend } from "./mockBackend";

describe("MockBackend meter settings (AC-1HHT01.4)", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("refuses to read without a connected meter", async () => {
    const b = new MockBackend();
    await expect(b.meterSettings()).rejects.toMatchObject({ kind: "usage" });
    expect(b.meterSettingsReads()).toBe(0);
  });

  it("returns every field of the demo meter with a tag and counts the reads", async () => {
    const b = new MockBackend();
    await b.connect("demo:load-5v-1a", 50, () => {});
    const s = await b.meterSettings();
    expect(s.fields.map((f) => f.key)).toEqual([
      "language",
      "brightness",
      "dim_timeout",
      "startup_logo",
      "theme",
      "auto_rotate",
      "rotation",
      "sample_rate",
      "voltage_cal",
      "current_cal",
      "offset_cal",
    ]);
    const value = (k: string) => s.fields.find((f) => f.key === k)?.value;
    expect(value("language")).toBe("English");
    expect(value("brightness")).toBe("50%");
    expect(value("theme")).toBe("blue");
    expect(value("rotation")).toBe("90°");
    expect(value("sample_rate")).toBe("50 SPS");
    for (const f of s.fields) {
      expect(f.label.length).toBeGreaterThan(0);
      expect(f.tag).toBe("inferred");
    }
    await b.meterSettings();
    expect(b.meterSettingsReads()).toBe(2);
    await b.disconnect();
  });
});
