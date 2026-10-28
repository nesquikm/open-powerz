/** Meter settings over Tauri IPC (FR 1HHT01, AC .4). */

import { beforeEach, describe, expect, it, vi } from "vitest";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => {
  class Channel {
    onmessage: (msg: unknown) => void = () => {};
  }
  return { invoke, Channel, isTauri: () => true };
});

import { meterSettings, type MeterSettings } from "./backend";

beforeEach(() => {
  invoke.mockReset();
});

describe("meterSettings (AC-1HHT01.4)", () => {
  it("reads the settings through the meter_settings command", async () => {
    const reply: MeterSettings = {
      fields: [{ key: "language", label: "Language", value: "English", tag: "inferred" }],
    };
    invoke.mockResolvedValue(reply);
    await expect(meterSettings()).resolves.toEqual(reply);
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(invoke).toHaveBeenCalledWith("meter_settings");
  });

  it("passes a refused read on as the backend error", async () => {
    const err = { kind: "protocol", message: "SETTINGS block B: CRC mismatch", hint: null };
    invoke.mockRejectedValue(err);
    await expect(meterSettings()).rejects.toEqual(err);
  });
});
