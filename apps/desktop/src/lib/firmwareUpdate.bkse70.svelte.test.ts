/**
 * FR BKSE70 (AC .5): an update cannot start on a meter listed in the old (2.1)
 * bootloader. Update is blocked and the old-bootloader message shows instead: not
 * supported yet, unplug to return to the application firmware, and the how-to link.
 */

import { describe, expect, it, vi } from "vitest";
import type { FirmwareFileInfo, FlashDone } from "./backend";
import {
  FirmwareUpdate,
  type FirmwareMeter,
  type FirmwareUpdateApi,
} from "./firmwareUpdate.svelte";

/** The how-to section the message links, written out independently. */
const HOWTO_URL =
  "https://github.com/nesquikm/open-powerz/blob/main/docs/how-to/update-firmware.md#old-bootloader";

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

/** A KM003C listed on the old bootloader (never connected, so no versions). */
const OLD_BOOTLOADER: FirmwareMeter = {
  id: "usb:1:9",
  model: "KM003C",
  serial: "SERIAL-BKSE-0009",
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

describe("an update on a meter listed in the old bootloader (AC-BKSE70.5)", () => {
  it("is blocked even with a file that fits the model", async () => {
    const fw = await withFile(api());
    expect(fw.canUpdate(OLD_BOOTLOADER)).toBe(false);
  });

  it("shows the old-bootloader message as the reason", async () => {
    const fw = await withFile(api());
    const why = fw.updateBlocker(OLD_BOOTLOADER) ?? "";
    expect(why.toLowerCase()).toContain("not supported yet");
    expect(why.toLowerCase()).toContain("unplug");
    expect(why).toContain(HOWTO_URL);
  });

  it("does not open the confirmation", async () => {
    const fw = await withFile(api());
    fw.requestUpdate(OLD_BOOTLOADER);
    expect(fw.phase).toBe("idle");
  });

  it("never reaches the backend, even with the version typed", async () => {
    const a = api();
    const fw = await withFile(a);
    fw.requestUpdate(OLD_BOOTLOADER);
    fw.confirmation = "2.0.7";
    await fw.start(OLD_BOOTLOADER);
    expect(a.flash).not.toHaveBeenCalled();
  });
});
