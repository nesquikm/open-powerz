/**
 * FR AXHK2W (AC-AXHK2W.11): the chart asks the backend where the session history's full
 * resolution starts, for its fold tick.
 *
 * API pinned here, beyond the FR's Technical Design:
 *
 * - `historyFoldBoundary(): Promise<number | null>` in `backend.ts` (and on the
 *   `meterBackend(id)` handle, sending `deviceId`): the Tauri command
 *   `history_fold_boundary`; `null` while nothing is folded;
 * - `MockOptions.historyFullSamples?: number | null`: the demo backend's full-resolution
 *   limit (none by default, so it never folds). Past it, `historyFoldBoundary()` answers
 *   a time after the session start and at or before the oldest of the newest
 *   `historyFullSamples` samples.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => {
  class Channel {
    onmessage: (msg: unknown) => void = () => {};
  }
  return { invoke, Channel, isTauri: () => true };
});

import { historyFoldBoundary, meterBackend } from "./backend";
import { MockBackend } from "./mockBackend";

beforeEach(() => {
  invoke.mockReset();
});

describe("history_fold_boundary command (AC-AXHK2W.11)", () => {
  it("answers the backend's boundary", async () => {
    invoke.mockResolvedValue(123_456);
    await expect(historyFoldBoundary()).resolves.toBe(123_456);
    expect(invoke.mock.calls[0]?.[0]).toBe("history_fold_boundary");
  });

  it("answers null while nothing is folded", async () => {
    invoke.mockResolvedValue(null);
    await expect(historyFoldBoundary()).resolves.toBeNull();
  });

  it("goes to the given meter", async () => {
    invoke.mockResolvedValue(null);
    await meterBackend("demo:load-5v-1a").historyFoldBoundary();
    expect(invoke).toHaveBeenCalledWith(
      "history_fold_boundary",
      expect.objectContaining({ deviceId: "demo:load-5v-1a" }),
    );
  });
});

let backend: MockBackend | null = null;
afterEach(async () => {
  await backend?.disconnect().catch(() => {});
  backend = null;
});

describe("MockBackend fold boundary (AC-AXHK2W.11)", () => {
  it("never folds by default", async () => {
    backend = new MockBackend(null, {});
    await backend.connect("demo:load-5v-1a", 1000, () => {});
    await backend.seedSyntheticHistory(20_000);
    await expect(backend.historyFoldBoundary()).resolves.toBeNull();
  });

  it("reports a boundary past its configured limit", async () => {
    backend = new MockBackend(null, { historyFullSamples: 5_000 });
    await backend.connect("demo:load-5v-1a", 1000, () => {});
    await backend.seedSyntheticHistory(4_000);
    await expect(backend.historyFoldBoundary()).resolves.toBeNull();
    // Samples one per ms from 0: the newest 5 000 of 20 000 start at 15 000 ms.
    await backend.seedSyntheticHistory(20_000);
    const boundary = await backend.historyFoldBoundary();
    expect(boundary).not.toBeNull();
    expect(boundary).toBeGreaterThan(0);
    expect(boundary).toBeLessThanOrEqual(15_000);
  });
});
