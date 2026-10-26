/**
 * FR V9SK55 (AC .2–.5): choosing a firmware file goes to the backend's `firmware_choose`
 * command (its open dialog), and the update to `firmware_flash`, which streams each stage
 * over a channel and resolves to the outcome or rejects with the backend's error.
 */

import { beforeEach, describe, expect, it, vi } from "vitest";

const { invoke, channels } = vi.hoisted(() => ({
  invoke: vi.fn(),
  channels: [] as { onmessage: (msg: unknown) => void }[],
}));

vi.mock("@tauri-apps/api/core", () => {
  class Channel {
    onmessage: (msg: unknown) => void = () => {};
    constructor() {
      channels.push(this);
    }
  }
  return { invoke, Channel, isTauri: () => true };
});

import { firmwareChoose, firmwareFlash, type FlashDone, type FlashStage } from "./backend";

beforeEach(() => {
  invoke.mockReset();
  channels.length = 0;
});

describe("firmwareChoose", () => {
  it("invokes firmware_choose and returns the chosen path", async () => {
    invoke.mockResolvedValue("/fw/KM003C_V2.0.7.mencrypt");
    await expect(firmwareChoose()).resolves.toBe("/fw/KM003C_V2.0.7.mencrypt");
    expect(invoke).toHaveBeenCalledWith("firmware_choose");
  });

  it("resolves to null when the dialog was cancelled", async () => {
    invoke.mockResolvedValue(null);
    await expect(firmwareChoose()).resolves.toBeNull();
  });
});

describe("firmwareFlash", () => {
  it("invokes firmware_flash with the device, file and typed confirmation", async () => {
    const done: FlashDone = { outcome: "verified", version: "2.0.7" };
    invoke.mockResolvedValue(done);
    const seen: FlashStage[] = [];
    await expect(
      firmwareFlash("usb:1:4", "/fw/a.mencrypt", "2.0.7", (s) => seen.push(s)),
    ).resolves.toEqual(done);
    expect(invoke).toHaveBeenCalledTimes(1);
    const [command, args] = invoke.mock.calls[0] as [string, Record<string, unknown>];
    expect(command).toBe("firmware_flash");
    expect(args).toMatchObject({
      deviceId: "usb:1:4",
      path: "/fw/a.mencrypt",
      confirmation: "2.0.7",
    });
    expect(args.onProgress).toBe(channels.at(-1));
  });

  it("hands each stage from the channel to the caller in order", async () => {
    let resolve: (d: FlashDone) => void = () => {};
    invoke.mockReturnValue(new Promise<FlashDone>((r) => (resolve = r)));
    const seen: FlashStage[] = [];
    const result = firmwareFlash("usb:1:4", "/fw/a.mencrypt", "2.0.7", (s) => seen.push(s));
    await Promise.resolve();
    const stages: FlashStage[] = [
      { stage: "switching" },
      { stage: "erasing" },
      { stage: "writing", done: 0, total: 8160 },
      { stage: "writing", done: 8160, total: 8160 },
      { stage: "verifying" },
    ];
    for (const s of stages) channels.at(-1)?.onmessage(s);
    resolve({ outcome: "power_cycle_needed" });
    await expect(result).resolves.toEqual({ outcome: "power_cycle_needed" });
    expect(seen).toEqual(stages);
  });

  it("passes a failure through as the backend's error", async () => {
    const err = {
      kind: "protocol",
      message: "erase error: no ACCEPT after the erase",
      hint: "Hold Back while plugging the meter in, then run the update again.",
    };
    invoke.mockRejectedValue(err);
    await expect(firmwareFlash("usb:1:4", "/fw/a.mencrypt", "2.0.7", () => {})).rejects.toEqual(
      err,
    );
  });
});
