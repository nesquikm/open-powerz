/**
 * FR DJYNRP (AC .7): the desktop update keeps refusing a meter listed on the
 * old (2.1) bootloader, and the reason shown is the updated message that names
 * the CLI's experimental flag.
 */

import { describe, expect, it, vi } from "vitest";
import type { FirmwareFileInfo, FlashDone } from "./backend";
import {
  FirmwareUpdate,
  type FirmwareMeter,
  type FirmwareUpdateApi,
} from "./firmwareUpdate.svelte";

const FLAG = "--experimental-old-bootloader";

const INFO: FirmwareFileInfo = {
  vendor: "ChargerLab",
  container_version: "V1.0",
  model: "KM003C",
  version: "2.0.7",
  date: "2026.9.29",
  crc: "A175E4A0",
  size: 12340,
  stored_size: 12340,
  release_notes_en: "1. Synthetic note.",
};

const OLD_BOOTLOADER: FirmwareMeter = {
  id: "usb:1:9",
  model: "KM003C",
  serial: "SERIAL-DJYN-0009",
  mode: "legacy_bootloader",
  firmwareVersion: null,
  bootloaderVersion: null,
};

function api(): FirmwareUpdateApi {
  return {
    choose: vi.fn(async () => "/fw/KM003C_V2.0.7.mencrypt"),
    inspect: vi.fn(async () => INFO),
    flash: vi.fn(async () => ({ outcome: "verified", version: "2.0.7" }) as FlashDone),
    cancelFlash: vi.fn(async () => {}),
  };
}

async function withFile(a: FirmwareUpdateApi): Promise<FirmwareUpdate> {
  const fw = new FirmwareUpdate(a);
  await fw.chooseFile();
  return fw;
}

describe("the desktop update on the old bootloader (AC-DJYNRP.7)", () => {
  it("stays blocked", async () => {
    const fw = await withFile(api());
    expect(fw.canUpdate(OLD_BOOTLOADER)).toBe(false);
  });

  it("shows the updated message naming the CLI flag", async () => {
    const fw = await withFile(api());
    expect(fw.updateBlocker(OLD_BOOTLOADER) ?? "").toContain(FLAG);
  });

  it("never reaches the backend", async () => {
    const a = api();
    const fw = await withFile(a);
    fw.requestUpdate(OLD_BOOTLOADER);
    fw.confirmation = "2.0.7";
    await fw.start(OLD_BOOTLOADER);
    expect(a.flash).not.toHaveBeenCalled();
  });
});
