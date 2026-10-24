/**
 * The firmware file inspection call goes to the backend's `firmware_inspect`
 * command (FR V9SK53, AC .3).
 */

import { beforeEach, describe, expect, it, vi } from "vitest";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => {
  class Channel {
    onmessage: (msg: unknown) => void = () => {};
  }
  return { invoke, Channel, isTauri: () => true };
});

import { firmwareInspect, type FirmwareFileInfo } from "./backend";

const info: FirmwareFileInfo = {
  vendor: "ChargerLab",
  container_version: "V1.0",
  model: "KM003C",
  version: "2.0.7",
  date: "2026.9.29",
  crc: "A175E4A0",
  size: 456896,
  release_notes_en: "1. Synthetic note.",
};

beforeEach(() => {
  invoke.mockReset();
});

describe("firmwareInspect", () => {
  it("invokes firmware_inspect with the path and returns the metadata", async () => {
    invoke.mockResolvedValue(info);
    await expect(firmwareInspect("/tmp/fw.mencrypt")).resolves.toEqual(info);
    expect(invoke).toHaveBeenCalledWith("firmware_inspect", { path: "/tmp/fw.mencrypt" });
  });

  it("passes a refusal through as the backend's error", async () => {
    const err = { kind: "usage", message: "This is not a firmware file.", hint: null };
    invoke.mockRejectedValue(err);
    await expect(firmwareInspect("/tmp/x.bin")).rejects.toEqual(err);
  });
});
