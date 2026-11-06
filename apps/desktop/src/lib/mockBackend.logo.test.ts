/** The demo backend's startup logo (FR MEHQC0, AC .1, .3 to .8): it previews a synthetic
 * picture (`?logofile=small` one with a side below 48), keeps a logo area that upload and
 * Remove change, streams progress at most 20 times a second, fails at chunk k with
 * `?logofault=<k>`, refuses before the settings were read and, with
 * `?mock=logo-other-model`, on a KM002C. */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { LogoProgress } from "./backend";
import {
  MOCK_LOGO_FILE,
  MOCK_LOGO_SMALL_FILE,
  MockBackend,
  logoFaultFromUrl,
  logoFileFromUrl,
  mockModeFromUrl,
} from "./mockBackend";

const DEMO = "demo:load-5v-1a";

async function connected(
  mode: ConstructorParameters<typeof MockBackend>[0] = null,
  options?: ConstructorParameters<typeof MockBackend>[1],
) {
  const b = new MockBackend(mode, options);
  await b.connect(DEMO, 50, () => {});
  return b;
}

async function ready(
  mode: ConstructorParameters<typeof MockBackend>[0] = null,
  options?: ConstructorParameters<typeof MockBackend>[1],
) {
  const b = await connected(mode, options);
  await b.meterSettings();
  return b;
}

/** Runs `p` to its end on the fake clock. */
async function settle<T>(p: Promise<T>): Promise<T> {
  const out = p.then(
    (v) => ({ ok: true as const, v }),
    (e: unknown) => ({ ok: false as const, e }),
  );
  await vi.advanceTimersByTimeAsync(120_000);
  const r = await out;
  if (r.ok) return r.v;
  throw r.e;
}

function b64Length(s: string): number {
  return atob(s).length;
}

describe("MockBackend logo preview (AC-MEHQC0.1, .8)", () => {
  it("chooses the demo picture and previews it opaque at its real size", async () => {
    const b = new MockBackend(null, {});
    const path = await b.logoChoose();
    expect(path).toBe(MOCK_LOGO_FILE);
    const p = await b.logoPreview(path!, false);
    expect(p).toMatchObject({
      width: 100,
      height: 60,
      mode: "opaque",
      blob_size: 12_016,
      padded_size: 12_032,
      chunks: 47,
      small_side_warning: false,
    });
    expect(b64Length(p.rgba)).toBe(100 * 60 * 4);
  });

  it("previews the same picture in alpha mode with three bytes a pixel", async () => {
    const b = new MockBackend(null, {});
    const p = await b.logoPreview(MOCK_LOGO_FILE, true);
    expect(p).toMatchObject({
      width: 100,
      height: 60,
      mode: "alpha",
      blob_size: 18_016,
      chunks: 71,
    });
  });

  it("?logofile=small chooses a picture with a side below 48", async () => {
    const b = new MockBackend(null, { logoFile: "small" });
    const path = await b.logoChoose();
    expect(path).toBe(MOCK_LOGO_SMALL_FILE);
    const p = await b.logoPreview(path!, false);
    expect(p).toMatchObject({ width: 240, height: 24, small_side_warning: true });
  });
});

describe("MockBackend logo writes (AC-MEHQC0.3, .4, .6, .7, .8)", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("Read of a fresh demo meter is empty", async () => {
    const b = await ready();
    await expect(b.logoRead(DEMO, false)).resolves.toMatchObject({ status: "empty", image: null });
    await b.disconnect();
  });

  it("an upload reports chunks then read-back, then Read shows the logo", async () => {
    const b = await ready();
    const seen: LogoProgress[] = [];
    const written = await settle(b.logoUpload(DEMO, MOCK_LOGO_FILE, false, (p) => seen.push(p)));
    expect(written).toEqual({
      duration_before: "off",
      duration_set: "3 s",
      settings: "needs_menu_save",
      startup_duration: null,
    });
    const chunks = seen.filter((p) => p.step === "chunks");
    const reads = seen.filter((p) => p.step === "read_back");
    expect(chunks.length).toBeGreaterThan(1);
    expect(reads.length).toBeGreaterThan(0);
    expect(chunks.every((p) => p.total === 47)).toBe(true);
    expect(seen.findIndex((p) => p.step === "read_back")).toBeGreaterThan(
      seen.length - 1 - [...seen].reverse().findIndex((p) => p.step === "chunks"),
    );
    const area = await b.logoRead(DEMO, true);
    expect(area).toMatchObject({ status: "logo", width: 100, height: 60, mode: "opaque" });
    expect(b64Length(area.image!)).toBe(100 * 60 * 4);
    await b.disconnect();
  });

  it("sends progress at most 20 times a second, 50 ms apart", async () => {
    const b = await ready();
    const at: number[] = [];
    await settle(b.logoUpload(DEMO, MOCK_LOGO_FILE, true, () => at.push(Date.now())));
    expect(at.length).toBeGreaterThan(2);
    for (let i = 1; i < at.length; i++) expect(at[i]! - at[i - 1]!).toBeGreaterThanOrEqual(50);
    const t0 = at[0]!;
    const perSecond = new Map<number, number>();
    for (const t of at) {
      const s = Math.floor((t - t0) / 1000);
      perSecond.set(s, (perSecond.get(s) ?? 0) + 1);
    }
    for (const n of perSecond.values()) expect(n).toBeLessThanOrEqual(20);
    await b.disconnect();
  });

  it("the write takes long enough for the view to show its progress", async () => {
    const b = await ready();
    let done = false;
    const p = b.logoUpload(DEMO, MOCK_LOGO_FILE, false, () => {}).then(() => (done = true));
    await vi.advanceTimersByTimeAsync(300);
    expect(done).toBe(false);
    await settle(p);
    expect(done).toBe(true);
    await b.disconnect();
  });

  it("?logofault=7 fails at chunk 7 and names it", async () => {
    const b = await ready(null, { logoFault: 7 });
    const seen: LogoProgress[] = [];
    const err = await settle(b.logoUpload(DEMO, MOCK_LOGO_FILE, false, (p) => seen.push(p))).catch(
      (e: unknown) => e,
    );
    expect(err).toMatchObject({ message: expect.stringContaining("chunk 7") });
    expect(seen.every((p) => p.step === "chunks" && p.done <= 7)).toBe(true);
    await b.disconnect();
  });

  it("after a failure at chunk k > 0, Read reports the uploaded size: chunks 0..k-1 landed", async () => {
    const b = await ready(null, { logoFault: 7 });
    await settle(b.logoUpload(DEMO, MOCK_LOGO_FILE, false, () => {})).catch((e: unknown) => e);
    // Chunk 0 carries the header, so the meter reports the logo the header describes.
    await expect(b.logoRead(DEMO, false)).resolves.toMatchObject({
      status: "logo",
      width: 100,
      height: 60,
      mode: "opaque",
    });
    await b.disconnect();
  });

  it("Remove brings the area back to empty", async () => {
    const b = await ready();
    await settle(b.logoUpload(DEMO, MOCK_LOGO_FILE, false, () => {}));
    const seen: LogoProgress[] = [];
    await settle(b.logoRemove(DEMO, (p) => seen.push(p)));
    expect(seen.length).toBeGreaterThan(0);
    expect((await b.logoRead(DEMO, false)).status).toBe("empty");
    await b.disconnect();
  });

  it("refuses an upload or Remove before the settings were read", async () => {
    const b = await connected();
    const up = await settle(b.logoUpload(DEMO, MOCK_LOGO_FILE, false, () => {})).catch(
      (e: unknown) => e,
    );
    expect(up).toMatchObject({ kind: "usage", message: expect.stringMatching(/settings/i) });
    const rm = await settle(b.logoRemove(DEMO, () => {})).catch((e: unknown) => e);
    expect(rm).toMatchObject({ kind: "usage", message: expect.stringMatching(/settings/i) });
    await b.disconnect();
  });

  it("refuses without a connected meter", async () => {
    const b = new MockBackend(null, {});
    await expect(b.logoRead(DEMO, false)).rejects.toMatchObject({ kind: "usage" });
    await expect(b.logoUpload(DEMO, MOCK_LOGO_FILE, false, () => {})).rejects.toMatchObject({
      kind: "usage",
    });
  });

  it("logo-other-model connects a KM002C and refuses its logo writes", async () => {
    const b = new MockBackend("logo-other-model", {});
    const events: { type: string; data?: { model?: string } }[] = [];
    await b.connect(DEMO, 50, (e) => events.push(e as (typeof events)[number]));
    expect(events.find((e) => e.type === "status")?.data?.model).toBe("KM002C");
    await b.meterSettings();
    const err = await settle(b.logoUpload(DEMO, MOCK_LOGO_FILE, false, () => {})).catch(
      (e: unknown) => e,
    );
    expect(err).toMatchObject({ message: expect.stringContaining("KM003C") });
    await b.disconnect();
  });
});

describe("MockBackend startup duration (AC-MEHQC0.5)", () => {
  it("the existing settings write takes startup_duration 1..3 and shows 1/3/5 s", async () => {
    const b = await ready();
    const value = (s: { fields: { key: string; value: string }[] }) =>
      s.fields.find((f) => f.key === "startup_logo")?.value;
    expect(value(await b.meterSettings())).toBe("off");
    expect(value(await b.meterSettingsWrite({ startup_duration: 2 }))).toBe("3 s");
    expect(value(await b.meterSettingsWrite({ startup_duration: 1 }))).toBe("1 s");
    expect(value(await b.meterSettingsWrite({ startup_duration: 3 }))).toBe("5 s");
    await expect(b.meterSettingsWrite({ startup_duration: 4 })).rejects.toMatchObject({
      kind: "usage",
    });
    await b.disconnect();
  });

  it("an upload reports the duration the settings held before", async () => {
    vi.useFakeTimers();
    try {
      const b = await ready();
      await b.meterSettingsWrite({ startup_duration: 2 });
      const written = await settle(b.logoUpload(DEMO, MOCK_LOGO_FILE, false, () => {}));
      expect(written).toEqual({
        duration_before: "3 s",
        duration_set: "3 s",
        settings: "needs_menu_save",
        startup_duration: null,
      });
      await b.disconnect();
    } finally {
      vi.useRealTimers();
    }
  });
});

describe("logo URL options", () => {
  it("reads ?logofault=<k> and ?logofile=small and ignores anything else", () => {
    expect(logoFaultFromUrl("?logofault=7")).toBe(7);
    expect(logoFaultFromUrl("?mock=x&logofault=0")).toBe(0);
    expect(logoFaultFromUrl("?logofault=-1")).toBeNull();
    expect(logoFaultFromUrl("?logofault=abc")).toBeNull();
    expect(logoFaultFromUrl("")).toBeNull();
    expect(logoFaultFromUrl(undefined)).toBeNull();
    expect(logoFileFromUrl("?logofile=small")).toBe("small");
    expect(logoFileFromUrl("?logofile=big")).toBeNull();
    expect(logoFileFromUrl(undefined)).toBeNull();
  });

  it("knows the logo-other-model mode", () => {
    expect(mockModeFromUrl("?mock=logo-other-model")).toBe("logo-other-model");
  });
});
