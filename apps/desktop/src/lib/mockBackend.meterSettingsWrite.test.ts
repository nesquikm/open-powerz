/** The demo backend's settings write (FR JKWT3W, AC .3, .4 and .6): the demo meter
 * keeps settings rows that a write changes, refuses keys that are not writable and
 * values out of range, and `settingsFault: "mismatch"` keeps the old values so the
 * re-read does not match (the error names the field). */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { MockBackend, settingsFaultFromUrl } from "./mockBackend";
import type { MeterSettings } from "./backend";

async function connected(options?: ConstructorParameters<typeof MockBackend>[1]) {
  const b = new MockBackend(null, options);
  await b.connect("demo:load-5v-1a", 50, () => {});
  return b;
}

function value(s: MeterSettings, key: string): string | undefined {
  return s.fields.find((f) => f.key === key)?.value;
}

describe("MockBackend settings write (AC-JKWT3W.6)", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("refuses to write without a connected meter", async () => {
    const b = new MockBackend(null, {});
    await expect(b.meterSettingsWrite({ brightness: 80 })).rejects.toMatchObject({
      kind: "usage",
    });
    expect(b.meterSettingsWrites()).toBe(0);
  });

  it("changes the written fields and keeps the rest", async () => {
    const b = await connected({});
    const before = await b.meterSettings();
    const after = await b.meterSettingsWrite({
      brightness: 80,
      theme: 2,
      rotation: 2,
      language: 0,
      dim_timeout: 1,
      sample_rate: 1,
    });
    expect(value(after, "brightness")).toBe("80%");
    expect(value(after, "theme")).toBe("pink");
    expect(value(after, "rotation")).toBe("180°");
    expect(value(after, "language")).toBe("Chinese");
    expect(value(after, "dim_timeout")).toBe("15 s");
    expect(value(after, "sample_rate")).toBe("10 SPS");
    for (const k of ["startup_logo", "auto_rotate", "voltage_cal", "current_cal", "offset_cal"]) {
      expect(value(after, k)).toBe(value(before, k));
    }
    expect(after.fields.map((f) => f.key)).toEqual(before.fields.map((f) => f.key));
    // A later read shows the change too.
    expect(value(await b.meterSettings(), "brightness")).toBe("80%");
    expect(b.meterSettingsWrites()).toBe(1);
    await b.disconnect();
  });

  it("refuses keys that are not writable and values out of range", async () => {
    const b = await connected({});
    const bad: Record<string, number>[] = [
      { voltage_cal: 1 },
      { offset_cal: 0 },
      { auto_rotate: 0 },
      { startup_logo: 1 },
      { factory_reset: 1 },
      { brightness: 0 },
      { brightness: 101 },
      { brightness: 50.5 },
      { theme: 7 },
      { language: 2 },
      { rotation: -1 },
      { dim_timeout: 4 },
      { sample_rate: 4 },
      {},
    ];
    for (const changes of bad) {
      await expect(
        b.meterSettingsWrite(changes as Parameters<MockBackend["meterSettingsWrite"]>[0]),
      ).rejects.toMatchObject({ kind: "usage" });
    }
    expect(b.meterSettingsWrites()).toBe(0);
    expect(value(await b.meterSettings(), "brightness")).toBe("50%");
    await b.disconnect();
  });

  it("with the mismatch fault keeps the old values and names the field", async () => {
    const b = await connected({ settingsFault: "mismatch" });
    await expect(b.meterSettingsWrite({ brightness: 80 })).rejects.toMatchObject({
      kind: "protocol",
      message: expect.stringContaining("rightness"),
    });
    expect(b.meterSettingsWrites()).toBe(1);
    expect(value(await b.meterSettings(), "brightness")).toBe("50%");
    await b.disconnect();
  });

  it("reads the fault from the page URL", () => {
    expect(settingsFaultFromUrl("?settingsfault=mismatch")).toBe("mismatch");
    expect(settingsFaultFromUrl("?settingsfault=other")).toBeNull();
    expect(settingsFaultFromUrl("")).toBeNull();
    expect(settingsFaultFromUrl(undefined)).toBeNull();
  });

  it("is one of a meter's commands", async () => {
    const b = await connected({});
    const meter = b.meter("demo:load-5v-1a");
    const after = await meter.meterSettingsWrite({ brightness: 33 });
    expect(value(after, "brightness")).toBe("33%");
    await b.disconnect();
  });
});
