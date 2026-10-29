/** Meter settings writes over Tauri IPC (FR JKWT3W, AC .5): the panel's changes
 * travel by field key with their codes; the answer is the block as read back. */

import { beforeEach, describe, expect, it, vi } from "vitest";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => {
  class Channel {
    onmessage: (msg: unknown) => void = () => {};
  }
  return { invoke, Channel, isTauri: () => true };
});

import {
  currentMeterCommands,
  meterBackend,
  meterSettingsWrite,
  type MeterSettings,
  type SettingChanges,
} from "./backend";

beforeEach(() => {
  invoke.mockReset();
});

const AFTER: MeterSettings = {
  fields: [{ key: "brightness", label: "Brightness", value: "80%", tag: "confirmed" }],
};

describe("meterSettingsWrite (AC-JKWT3W.5)", () => {
  it("writes the changes through the meter_settings_write command", async () => {
    invoke.mockResolvedValue(AFTER);
    const changes: SettingChanges = { brightness: 80, theme: 2 };
    await expect(meterSettingsWrite(changes)).resolves.toEqual(AFTER);
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(invoke).toHaveBeenCalledWith("meter_settings_write", {
      changes: { brightness: 80, theme: 2 },
    });
  });

  it("is one of the current meter's commands", () => {
    expect(currentMeterCommands.meterSettingsWrite).toBe(meterSettingsWrite);
  });

  it("sends the device id for a given meter", async () => {
    invoke.mockResolvedValue(AFTER);
    await meterBackend("usb:1-2").meterSettingsWrite({ rotation: 3 });
    expect(invoke).toHaveBeenCalledWith("meter_settings_write", {
      deviceId: "usb:1-2",
      changes: { rotation: 3 },
    });
  });

  it("passes a mismatch on as the backend error, once", async () => {
    const err = {
      kind: "protocol",
      message: "After the write the meter shows brightness 50, expected 80.",
      hint: null,
    };
    invoke.mockRejectedValue(err);
    await expect(meterSettingsWrite({ brightness: 80 })).rejects.toEqual(err);
    expect(invoke).toHaveBeenCalledTimes(1);
  });
});
