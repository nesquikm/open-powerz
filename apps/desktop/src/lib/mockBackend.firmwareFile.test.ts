/**
 * The mock backend answers firmware file inspection like the real one
 * (FR V9SK53, AC .3), with synthetic metadata only. Like the real 2.0.7 file, its file
 * stores 16 bytes past its image, and its update counts the stored bytes (FR VQCKC6,
 * AC .5).
 */

import { afterEach, describe, expect, it, vi } from "vitest";
import type { FlashEvent } from "./backend";
import { MockBackend } from "./mockBackend";

describe("MockBackend.firmwareInspect", () => {
  it("returns synthetic KM003C metadata with English notes", async () => {
    const info = await new MockBackend().firmwareInspect("/demo/KM003C_V2.0.7.mencrypt");
    expect(info.model).toBe("KM003C");
    expect(info.version).toMatch(/^\d+\.\d+\.\d+$/);
    expect(info.vendor).toBe("ChargerLab");
    expect(info.crc).toMatch(/^[0-9A-F]{8}$/);
    expect(info.size).toBeGreaterThan(0);
    expect(info.release_notes_en.trim().length).toBeGreaterThan(0);
  });

  it("stores 16 bytes past its declared image, like the real file (AC-VQCKC6.5)", async () => {
    const info = await new MockBackend().firmwareInspect("/demo/KM003C_V2.0.7.mencrypt");
    expect(info.size).toBe(456896);
    expect(info.stored_size).toBe(456912);
  });
});

describe("MockBackend firmware update counts the stored bytes (AC-VQCKC6.5)", () => {
  afterEach(() => vi.useRealTimers());

  it("the Writing stage totals and ends at the file's stored size", async () => {
    vi.useFakeTimers();
    const backend = new MockBackend();
    const path = await backend.firmwareChoose();
    if (path === null) throw new Error("the mock chose no file");
    const info = await backend.firmwareInspect(path);
    const stages: FlashEvent[] = [];
    const p = backend.firmwareFlash("demo:load-5v-1a", path, "2.0.7", (s) => stages.push(s));
    await vi.runAllTimersAsync();
    await p;
    const writes = stages.flatMap((s) => (s.stage === "writing" ? [s] : []));
    expect(writes.length).toBeGreaterThan(1);
    expect(writes.every((w) => w.total === info.stored_size)).toBe(true);
    expect(writes.at(-1)?.done).toBe(456912);
  });
});
