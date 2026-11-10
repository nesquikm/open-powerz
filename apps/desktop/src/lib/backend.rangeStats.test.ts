/**
 * FR AXHK2T (AC-AXHK2T.2): the ruler asks the backend for its values over Tauri IPC,
 * from the raw samples: `range_stats` for a meter's session, `recording_file_range_stats`
 * for an opened recording; both answer a `RangeStats` or `null`.
 */

import { beforeEach, describe, expect, it, vi } from "vitest";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => {
  class Channel {
    onmessage: (msg: unknown) => void = () => {};
  }
  return { invoke, Channel, isTauri: () => true };
});

import { meterBackend, rangeStats, recordingFileRangeStats, type RangeStats } from "./backend";

const answer: RangeStats = {
  samples: 3601,
  charge_ah: 1,
  energy_wh: 5,
  vbus_avg_v: 5,
  ibus_avg_a: 1,
  power_avg_w: 5,
  vbus_min_v: 5,
  vbus_max_v: 5,
  ibus_min_a: 1,
  ibus_max_a: 1,
  power_min_w: 5,
  power_max_w: 5,
};

beforeEach(() => {
  invoke.mockReset();
  invoke.mockResolvedValue(answer);
});

describe("ruler commands (AC-AXHK2T.2)", () => {
  it("range_stats carries whole milliseconds and answers the backend's stats", async () => {
    await expect(rangeStats(0.4, 3_600_000.6)).resolves.toEqual(answer);
    const [command, args] = invoke.mock.calls[0] as [string, { t0Ms: number; t1Ms: number }];
    expect(command).toBe("range_stats");
    expect(Number.isInteger(args.t0Ms)).toBe(true);
    expect(Number.isInteger(args.t1Ms)).toBe(true);
    expect(args.t0Ms).toBeLessThanOrEqual(0.4);
    expect(args.t1Ms).toBeGreaterThanOrEqual(3_600_000);
  });

  it("range_stats goes to the given meter", async () => {
    await meterBackend("demo:load-5v-1a").rangeStats(1_000, 2_000);
    expect(invoke).toHaveBeenCalledWith(
      "range_stats",
      expect.objectContaining({ deviceId: "demo:load-5v-1a", t0Ms: 1_000, t1Ms: 2_000 }),
    );
  });

  it("recording_file_range_stats takes no meter", async () => {
    await recordingFileRangeStats(10, 20);
    const [command, args] = invoke.mock.calls[0] as [string, Record<string, unknown>];
    expect(command).toBe("recording_file_range_stats");
    expect(args).toEqual({ t0Ms: 10, t1Ms: 20 });
  });

  it("an empty range answers null", async () => {
    invoke.mockResolvedValue(null);
    await expect(rangeStats(5, 6)).resolves.toBeNull();
  });
});
