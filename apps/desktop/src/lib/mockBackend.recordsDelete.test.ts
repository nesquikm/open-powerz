/** The demo backend's record delete (FR JKWT3V, AC .2, .3, .4 and .6): the demo
 * meter keeps a record table that a delete changes; the typed name (or "all") must
 * match; `recordsFault: "backup"` fails the backup and deletes nothing;
 * `recordsFault: "mismatch"` keeps the old table so the re-read does not match, and
 * the error names the backup. */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { MockBackend, recordsFaultFromUrl } from "./mockBackend";

async function connected(options?: ConstructorParameters<typeof MockBackend>[1]) {
  const b = new MockBackend(null, options);
  await b.connect("demo:load-5v-1a", 50, () => {});
  return b;
}

async function names(b: MockBackend): Promise<string[]> {
  return (await b.recordsList()).records.map((r) => r.name);
}

describe("MockBackend record delete (AC-JKWT3V.6)", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("refuses to delete without a connected meter", async () => {
    const b = new MockBackend(null);
    await expect(b.recordsDelete(1, "PHONE_5V")).rejects.toMatchObject({ kind: "usage" });
  });

  it("deletes one record: it leaves the list, the rest keep their order and renumber", async () => {
    const b = await connected();
    const before = await names(b);
    expect(before.length).toBeGreaterThanOrEqual(3);
    const res = await b.recordsDelete(2, before[1]!);
    const want = before.filter((_, i) => i !== 1);
    expect(res.records.map((r) => r.name)).toEqual(want);
    res.records.forEach((r, i) => expect(r.index).toBe(i + 1));
    expect(res.backup_dir.length).toBeGreaterThan(0);
    expect(res.backups).toHaveLength(1);
    expect(res.backups[0]).toContain(res.backup_dir);
    expect(await names(b)).toEqual(want);
    await b.disconnect();
  });

  it("deletes all records with the word all typed", async () => {
    const b = await connected();
    const before = await names(b);
    const res = await b.recordsDelete("all", "all");
    expect(res.records).toEqual([]);
    expect(res.backups).toHaveLength(before.length);
    expect(await names(b)).toEqual([]);
    await b.disconnect();
  });

  it("refuses a wrongly typed confirmation and keeps every record (AC-JKWT3V.3)", async () => {
    const b = await connected();
    const before = await names(b);
    for (const [target, typed] of [
      [1, before[1]!],
      [1, ""],
      [1, before[0]!.toLowerCase()],
      [1, "all"],
      ["all", before[0]!],
      ["all", "ALL"],
    ] as const) {
      await expect(b.recordsDelete(target, typed)).rejects.toMatchObject({ kind: "usage" });
    }
    expect(await names(b)).toEqual(before);
    await b.disconnect();
  });

  it("refuses an index that is not on the meter", async () => {
    const b = await connected();
    await expect(b.recordsDelete(99, "X")).rejects.toMatchObject({ kind: "usage" });
    await b.disconnect();
  });

  it("a failed backup deletes nothing (AC-JKWT3V.2)", async () => {
    const b = await connected({ recordsFault: "backup" });
    const before = await names(b);
    const err = await b.recordsDelete(1, before[0]!).catch((e: unknown) => e);
    expect(err).toMatchObject({ message: expect.stringMatching(/backup/i) });
    expect(await names(b)).toEqual(before);
    await b.disconnect();
  });

  it("a mismatching re-read is an error naming the backup and keeps the table (AC-JKWT3V.4)", async () => {
    const b = await connected({ recordsFault: "mismatch" });
    const before = await names(b);
    const err = (await b.recordsDelete(1, before[0]!).catch((e: unknown) => e)) as {
      message: string;
      hint: string | null;
    };
    expect(`${err.message} ${err.hint ?? ""}`).toMatch(/backup/i);
    expect(`${err.message} ${err.hint ?? ""}`).toMatch(/\//);
    expect(await names(b)).toEqual(before);
    await b.disconnect();
  });
});

describe("recordsFaultFromUrl", () => {
  it("reads ?recordsfault=backup|mismatch and ignores anything else", () => {
    expect(recordsFaultFromUrl("?recordsfault=backup")).toBe("backup");
    expect(recordsFaultFromUrl("?mock=replug&recordsfault=mismatch")).toBe("mismatch");
    expect(recordsFaultFromUrl("?recordsfault=other")).toBeNull();
    expect(recordsFaultFromUrl("")).toBeNull();
    expect(recordsFaultFromUrl(undefined)).toBeNull();
  });
});
