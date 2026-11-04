/**
 * FR XCCQAK: the firmware update keeps the labels the updated meter's entry took in the
 * device list (AC .1-.3), so a tester can read the bootloader label after the update.
 *
 * Store API under test (the view renders it):
 * - `firmware.noteListings(listings)`: the app calls it with the whole device list when an
 *   update starts and on every change of the list while the update holds the meter;
 * - `firmware.listedHistory`: the updated meter's listings in the order they were seen,
 *   one per label the closed device select shows (`deviceOptionLabel`), a label repeating
 *   only after a different one; emptied by a new update's start and by `dismiss()`.
 *
 * The view shows it as a "Device list" line: `deviceOptionLabel(l, hideSerial)` of each,
 * joined by " → ". All on a fake backend; nothing reaches a meter.
 */

import { describe, expect, it, vi } from "vitest";
import type { DeviceListing, FirmwareFileInfo, FlashDone, FlashEvent, UiError } from "./backend";
import {
  FirmwareUpdate,
  type FirmwareMeter,
  type FirmwareUpdateApi,
} from "./firmwareUpdate.svelte";
import { deviceOptionLabel } from "./format";

const INFO: FirmwareFileInfo = {
  vendor: "ChargerLab",
  container_version: "V1.0",
  model: "KM003C",
  version: "2.0.7",
  date: "2026.9.29",
  crc: "A175E4A0",
  size: 12340,
  release_notes_en: "1. Synthetic note.",
};

const SERIAL = "TEST-SERIAL-0042";

const METER: FirmwareMeter = {
  id: "usb:1:4",
  model: "KM003C",
  mode: "application",
  serial: SERIAL,
  firmwareVersion: "2.0.6",
  bootloaderVersion: "2.5",
};

/** The updated meter's entry at `address` in `mode`, labelled as the backend lists it. */
function meterAt(address: number, mode: "application" | "bootloader"): DeviceListing {
  const label = `KM003C — ${SERIAL}`;
  return {
    id: `usb:1:${address}`,
    label: mode === "bootloader" ? `${label} (bootloader mode)` : label,
    model: "KM003C",
    serial: SERIAL,
    bus: "1",
    address,
    demo_scenario: null,
    mode,
  };
}

/** Another real meter, never the updated one. */
const OTHER: DeviceListing = {
  id: "usb:1:12",
  label: "KM003C — OTHER-0001 (bootloader mode)",
  model: "KM003C",
  serial: "OTHER-0001",
  bus: "1",
  address: 12,
  demo_scenario: null,
  mode: "bootloader",
};

const DEMO: DeviceListing = {
  id: "demo:load-5v-1a",
  label: "Demo device — load-5v-1a",
  model: "KM003C",
  serial: "1234567",
  bus: null,
  address: null,
  demo_scenario: "load-5v-1a",
  mode: "application",
};

/** A flash the test drives. */
function fakeBackend() {
  let onStage: (s: FlashEvent) => void = () => {};
  let reject: (e: UiError) => void = () => {};
  let resolve: (d: FlashDone) => void = () => {};
  const api = {
    choose: vi.fn(async () => "/fw/KM003C_V2.0.7.mencrypt"),
    inspect: vi.fn(async () => INFO),
    flash: vi.fn(
      (_id: string, _path: string, _confirmation: string, stage: (s: FlashEvent) => void) => {
        onStage = stage;
        return new Promise<FlashDone>((res, rej) => {
          resolve = res;
          reject = rej;
        });
      },
    ),
    cancelFlash: vi.fn(async () => {}),
    flashStatus: vi.fn(async () => null),
    stopRecording: vi.fn(async () => {}),
    reconnect: vi.fn(async () => {}),
  } as FirmwareUpdateApi;
  return {
    api,
    stage: (s: FlashEvent) => onStage(s),
    reject: (e: UiError) => reject(e),
    resolve: (d: FlashDone) => resolve(d),
  };
}

async function settle(): Promise<void> {
  for (let i = 0; i < 5; i++) await Promise.resolve();
}

/** A running update of `METER`; the app has noted the list as it was at the start. */
async function running(
  api: FirmwareUpdateApi,
): Promise<{ fw: FirmwareUpdate; ended: Promise<void> }> {
  const fw = new FirmwareUpdate(api);
  await fw.chooseFile();
  fw.requestUpdate(METER);
  fw.confirmation = "2.0.7";
  const ended = fw.start(METER);
  expect(fw.phase).toBe("updating");
  fw.noteListings([meterAt(4, "application"), OTHER, DEMO]);
  return { fw, ended };
}

/** What the view's "Device list" line reads with the switch at `hidden`. */
function line(fw: FirmwareUpdate, hidden: boolean): string {
  return fw.listedHistory.map((l) => deviceOptionLabel(l, hidden)).join(" → ");
}

/** The application → bootloader → application round trip an update makes. */
function roundTrip(fw: FirmwareUpdate): void {
  fw.noteListings([OTHER, DEMO]); // the meter is gone while it jumps
  fw.noteListings([meterAt(4, "bootloader"), OTHER, DEMO]);
  fw.noteListings([meterAt(4, "bootloader"), OTHER, DEMO]);
  fw.noteListings([OTHER, DEMO]);
  fw.noteListings([meterAt(5, "application"), OTHER, DEMO]);
}

describe("the device list line (AC-XCCQAK.1)", () => {
  it("starts with the meter's label as listed when the update started", async () => {
    const be = fakeBackend();
    const { fw } = await running(be.api);
    expect(fw.listedHistory.map((l) => l.id)).toEqual(["usb:1:4"]);
    expect(line(fw, false)).toBe(`KM003C — ${SERIAL}`);
  });

  it("names each label in order while the update runs, joined by arrows", async () => {
    const be = fakeBackend();
    const { fw } = await running(be.api);
    be.stage({ stage: "switching" });
    fw.noteListings([OTHER, DEMO]);
    fw.noteListings([meterAt(4, "bootloader"), OTHER, DEMO]);
    expect(fw.phase).toBe("updating");
    expect(line(fw, false)).toBe(`KM003C — ${SERIAL} → KM003C — ${SERIAL} (bootloader mode)`);
  });

  it("keeps the whole line after the update is done", async () => {
    const be = fakeBackend();
    const { fw, ended } = await running(be.api);
    roundTrip(fw);
    be.resolve({ outcome: "verified", version: "2.0.7" });
    await ended;
    await settle();
    expect(fw.phase).toBe("done");
    expect(line(fw, false)).toBe(
      `KM003C — ${SERIAL} → KM003C — ${SERIAL} (bootloader mode) → KM003C — ${SERIAL}`,
    );
  });

  it("a label repeats only after a different one: the same list, or a new address in the same mode, adds nothing", async () => {
    const be = fakeBackend();
    const { fw } = await running(be.api);
    fw.noteListings([meterAt(4, "application"), OTHER, DEMO]);
    fw.noteListings([meterAt(6, "application"), DEMO]);
    expect(fw.listedHistory).toHaveLength(1);
    fw.noteListings([meterAt(6, "bootloader"), DEMO]);
    fw.noteListings([meterAt(7, "bootloader"), DEMO]);
    expect(fw.listedHistory.map((l) => l.mode)).toEqual(["application", "bootloader"]);
    fw.noteListings([meterAt(7, "application"), DEMO]);
    fw.noteListings([meterAt(7, "bootloader"), DEMO]);
    expect(fw.listedHistory.map((l) => l.mode)).toEqual([
      "application",
      "bootloader",
      "application",
      "bootloader",
    ]);
  });

  it("follows only the updated meter: other entries and a list without it add nothing", async () => {
    const be = fakeBackend();
    const { fw } = await running(be.api);
    fw.noteListings([OTHER, DEMO]);
    fw.noteListings([DEMO]);
    fw.noteListings([]);
    expect(fw.listedHistory.map((l) => l.id)).toEqual(["usb:1:4"]);
    expect(line(fw, false)).not.toContain("OTHER-0001");
    expect(line(fw, false)).not.toContain("Demo device");
  });

  it("holds no labels before any update", () => {
    const fw = new FirmwareUpdate(fakeBackend().api);
    fw.noteListings([meterAt(4, "application"), OTHER, DEMO]);
    expect(fw.listedHistory).toEqual([]);
  });
});

describe("each label is the select's own text (AC-XCCQAK.2)", () => {
  it("reads the bootloader entry with (bootloader mode)", async () => {
    const be = fakeBackend();
    const { fw } = await running(be.api);
    roundTrip(fw);
    const labels = fw.listedHistory.map((l) => deviceOptionLabel(l, false));
    expect(labels[1]).toBe(`KM003C — ${SERIAL} (bootloader mode)`);
  });

  it("with Hide serial numbers on, no serial appears in the line", async () => {
    const be = fakeBackend();
    const { fw } = await running(be.api);
    roundTrip(fw);
    const hidden = line(fw, true);
    expect(hidden).toBe("KM003C — Hidden → KM003C — Hidden (bootloader mode) → KM003C — Hidden");
    expect(hidden).not.toContain(SERIAL);
  });

  it("keeps the listings themselves, so the switch restyles the line", async () => {
    const be = fakeBackend();
    const { fw } = await running(be.api);
    roundTrip(fw);
    expect(line(fw, false)).toContain(SERIAL);
    expect(line(fw, true)).not.toContain(SERIAL);
    expect(fw.listedHistory.map((l) => l.mode)).toEqual([
      "application",
      "bootloader",
      "application",
    ]);
  });
});

describe("the line outlives the update until its outcome is closed (AC-XCCQAK.3)", () => {
  it("stays after a failed update", async () => {
    const be = fakeBackend();
    const { fw, ended } = await running(be.api);
    be.stage({ stage: "switching" });
    fw.noteListings([meterAt(4, "bootloader"), DEMO]);
    be.stage({ stage: "erasing" });
    be.reject({ kind: "protocol", message: "The erase failed.", hint: null });
    await ended;
    await settle();
    expect(fw.phase).toBe("failed");
    expect(line(fw, false)).toBe(`KM003C — ${SERIAL} → KM003C — ${SERIAL} (bootloader mode)`);
  });

  it("stays after a stopped update", async () => {
    const be = fakeBackend();
    const { fw, ended } = await running(be.api);
    be.stage({ stage: "switching" });
    fw.noteListings([meterAt(4, "bootloader"), DEMO]);
    be.reject({ kind: "cancelled", message: "The update was stopped.", hint: null });
    await ended;
    await settle();
    expect(fw.phase).toBe("stopped");
    expect(line(fw, false)).toBe(`KM003C — ${SERIAL} → KM003C — ${SERIAL} (bootloader mode)`);
  });

  it("is cleared when the outcome is closed", async () => {
    const be = fakeBackend();
    const { fw, ended } = await running(be.api);
    roundTrip(fw);
    be.resolve({ outcome: "verified", version: "2.0.7" });
    await ended;
    await settle();
    expect(fw.listedHistory).toHaveLength(3);
    fw.dismiss();
    expect(fw.phase).toBe("idle");
    expect(fw.listedHistory).toEqual([]);
    // Closed means closed: a later change of the list starts no new line.
    fw.noteListings([meterAt(5, "bootloader"), DEMO]);
    expect(fw.listedHistory).toEqual([]);
  });

  it("a new update starts the line afresh", async () => {
    const be = fakeBackend();
    const { fw, ended } = await running(be.api);
    roundTrip(fw);
    be.reject({ kind: "protocol", message: "The verify failed.", hint: null });
    await ended;
    await settle();
    expect(fw.listedHistory).toHaveLength(3);
    fw.dismiss();
    const again: FirmwareMeter = { ...METER, id: "usb:1:5" };
    fw.requestUpdate(again);
    fw.confirmation = "2.0.7";
    void fw.start(again);
    expect(fw.phase).toBe("updating");
    expect(fw.listedHistory).toEqual([]);
    fw.noteListings([meterAt(5, "application"), DEMO]);
    expect(line(fw, false)).toBe(`KM003C — ${SERIAL}`);
  });
});

/**
 * FR PGY704: the real backend ends the flash before the OS lists the meter in application
 * mode again, so the line keeps noting the updated meter after the update ends, until a
 * label in application mode closes it, or the outcome is closed.
 *
 * Store API under test: `firmware.notingListings`, true while an update runs and, once it
 * ended, until the line's last label is in application mode; false after `dismiss()`.
 */
describe("the line keeps noting the meter after the update ends (FR PGY704)", () => {
  const FULL = `KM003C — ${SERIAL} → KM003C — ${SERIAL} (bootloader mode) → KM003C — ${SERIAL}`;
  const TWO = `KM003C — ${SERIAL} → KM003C — ${SERIAL} (bootloader mode)`;

  /** A running update whose meter is listed in bootloader mode, not yet back. */
  async function inBootloader(api: FirmwareUpdateApi) {
    const r = await running(api);
    r.fw.noteListings([OTHER, DEMO]);
    r.fw.noteListings([meterAt(4, "bootloader"), OTHER, DEMO]);
    return r;
  }

  it("a meter listed back in application mode after the update ended completes the line (AC-PGY704.1)", async () => {
    const be = fakeBackend();
    const { fw, ended } = await inBootloader(be.api);
    be.resolve({ outcome: "verified", version: "2.0.7" });
    await ended;
    await settle();
    expect(fw.phase).toBe("done");
    expect(line(fw, false)).toBe(TWO);
    fw.noteListings([OTHER, DEMO]);
    fw.noteListings([meterAt(5, "application"), OTHER, DEMO]);
    expect(line(fw, false)).toBe(FULL);
    expect(line(fw, true)).toBe(
      "KM003C — Hidden → KM003C — Hidden (bootloader mode) → KM003C — Hidden",
    );
  });

  it("notes while the update runs and, once it is done, until a label in application mode closes the line (AC-PGY704.2)", async () => {
    const be = fakeBackend();
    const { fw, ended } = await inBootloader(be.api);
    expect(fw.notingListings).toBe(true);
    be.resolve({ outcome: "verified", version: "2.0.7" });
    await ended;
    await settle();
    expect(fw.notingListings).toBe(true);
    fw.noteListings([meterAt(5, "application"), OTHER, DEMO]);
    expect(fw.notingListings).toBe(false);
    // The line is closed: any later list leaves it as it is.
    fw.noteListings([meterAt(5, "bootloader"), OTHER, DEMO]);
    fw.noteListings([OTHER, DEMO]);
    fw.noteListings([meterAt(6, "application"), DEMO]);
    expect(line(fw, false)).toBe(FULL);
    expect(fw.listedHistory.map((l) => l.mode)).toEqual([
      "application",
      "bootloader",
      "application",
    ]);
  });

  it("a meter relisted in application mode before the end closes the line at the end (AC-PGY704.2)", async () => {
    const be = fakeBackend();
    const { fw, ended } = await running(be.api);
    roundTrip(fw);
    be.resolve({ outcome: "verified", version: "2.0.7" });
    await ended;
    await settle();
    expect(fw.notingListings).toBe(false);
    fw.noteListings([meterAt(5, "bootloader"), OTHER, DEMO]);
    expect(line(fw, false)).toBe(FULL);
  });

  it("keeps noting after a failed update until the meter is back in application mode (AC-PGY704.2)", async () => {
    const be = fakeBackend();
    const { fw, ended } = await inBootloader(be.api);
    be.reject({ kind: "protocol", message: "The erase failed.", hint: null });
    await ended;
    await settle();
    expect(fw.phase).toBe("failed");
    expect(fw.notingListings).toBe(true);
    fw.noteListings([meterAt(4, "bootloader"), DEMO]);
    expect(line(fw, false)).toBe(TWO);
    fw.noteListings([DEMO]);
    fw.noteListings([meterAt(8, "application"), DEMO]);
    expect(line(fw, false)).toBe(FULL);
    expect(fw.notingListings).toBe(false);
    fw.noteListings([meterAt(8, "bootloader"), DEMO]);
    expect(line(fw, false)).toBe(FULL);
  });

  it("keeps noting after a stopped update until the meter is back in application mode (AC-PGY704.2)", async () => {
    const be = fakeBackend();
    const { fw, ended } = await inBootloader(be.api);
    be.reject({ kind: "cancelled", message: "The update was stopped.", hint: null });
    await ended;
    await settle();
    expect(fw.phase).toBe("stopped");
    expect(fw.notingListings).toBe(true);
    fw.noteListings([meterAt(5, "application"), OTHER, DEMO]);
    expect(line(fw, false)).toBe(FULL);
    expect(fw.notingListings).toBe(false);
    fw.noteListings([meterAt(5, "bootloader"), OTHER, DEMO]);
    expect(line(fw, false)).toBe(FULL);
  });

  it("once the outcome is closed, no listing is noted until a new update starts (AC-PGY704.3)", async () => {
    const be = fakeBackend();
    const { fw, ended } = await inBootloader(be.api);
    be.resolve({ outcome: "verified", version: "2.0.7" });
    await ended;
    await settle();
    expect(fw.notingListings).toBe(true);
    fw.dismiss();
    expect(fw.phase).toBe("idle");
    expect(fw.notingListings).toBe(false);
    expect(fw.listedHistory).toEqual([]);
    fw.noteListings([meterAt(5, "application"), OTHER, DEMO]);
    fw.noteListings([meterAt(5, "bootloader"), OTHER, DEMO]);
    expect(fw.listedHistory).toEqual([]);
    expect(fw.notingListings).toBe(false);

    const again: FirmwareMeter = { ...METER, id: "usb:1:5" };
    fw.requestUpdate(again);
    fw.confirmation = "2.0.7";
    void fw.start(again);
    expect(fw.phase).toBe("updating");
    expect(fw.notingListings).toBe(true);
    expect(fw.listedHistory).toEqual([]);
    fw.noteListings([meterAt(5, "application"), DEMO]);
    expect(line(fw, false)).toBe(`KM003C — ${SERIAL}`);
  });
});
