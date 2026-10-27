/** The demo backend's offline records (FR 1HHT00, AC .5 and .6): a few synthetic
 * records, listed and downloaded only while a meter is connected. */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { MockBackend } from "./mockBackend";

describe("MockBackend offline records (AC-1HHT00.6)", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("refuses to list or download without a connected meter", async () => {
    const b = new MockBackend();
    await expect(b.recordsList()).rejects.toMatchObject({ kind: "usage" });
    await expect(b.recordDownload(1)).rejects.toMatchObject({ kind: "usage" });
    expect(b.recordDownloads()).toBe(0);
  });

  it("lists at least three records with every decoded column", async () => {
    const b = new MockBackend();
    await b.connect("demo:load-5v-1a", 50, () => {});
    const { records } = await b.recordsList();
    expect(records.length).toBeGreaterThanOrEqual(3);
    records.forEach((r, i) => {
      expect(r.index).toBe(i + 1);
      expect(r.name.length).toBeGreaterThan(0);
      expect(r.interval_s).toBeGreaterThan(0);
      expect(Number.isInteger(r.points)).toBe(true);
      expect(Number.isInteger(r.offset)).toBe(true);
      expect(typeof r.charge_ah).toBe("number");
      expect(typeof r.energy_wh).toBe("number");
    });
    expect(records.filter((r) => r.points > 0).length).toBeGreaterThanOrEqual(3);
    await b.disconnect();
  });

  it("downloads a record's samples, consistent with its entry, and counts downloads", async () => {
    const b = new MockBackend();
    await b.connect("demo:load-5v-1a", 50, () => {});
    const { records } = await b.recordsList();
    const entry = records.find((r) => r.points > 0)!;
    const rec = await b.recordDownload(entry.index);
    expect(rec.index).toBe(entry.index);
    expect(rec.name).toBe(entry.name);
    expect(rec.samples.length).toBe(entry.points);
    rec.samples.forEach((s, k) => expect(s.t_s).toBe(entry.interval_s * k));
    const last = rec.samples[rec.samples.length - 1]!;
    expect(last.charge_ah).toBeCloseTo(entry.charge_ah, 9);
    expect(last.energy_wh).toBeCloseTo(entry.energy_wh, 9);
    expect(b.recordDownloads()).toBe(1);
    await expect(b.recordDownload(999)).rejects.toMatchObject({ kind: "usage" });
    expect(b.recordDownloads()).toBe(1);
    await b.disconnect();
  });
});
