/**
 * The mock backend answers firmware file inspection like the real one
 * (FR V9SK53, AC .3), with synthetic metadata only.
 */

import { describe, expect, it } from "vitest";
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
});
