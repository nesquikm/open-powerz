/**
 * FR V9SK55 (AC .2–.5): the firmware view's state. The meter's mode and versions; the
 * chosen file's metadata; Update only for a file that passed inspection and fits the
 * meter's model; a typed confirmation of the target version with the keep-connected /
 * on-power warning; each stage with the write progress; the new version on success, the
 * error with the failed stage and the hold-Back recovery steps on failure.
 */

import { describe, expect, it, vi } from "vitest";
import type {
  ConnectedInfo,
  DeviceListing,
  FirmwareFileInfo,
  FlashDone,
  FlashEvent,
  UiError,
} from "./backend";
import {
  FLASH_STAGES,
  FirmwareUpdate,
  RECOVERY_STEPS,
  UPDATE_WARNING,
  firmwareMeterRows,
  flashMeter,
  replugPrompt,
  stageLabel,
  stageNameLabel,
  type FirmwareMeter,
  type FirmwareUpdateApi,
} from "./firmwareUpdate.svelte";

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

const METER: FirmwareMeter = {
  id: "usb:1:4",
  model: "KM003C",
  mode: "application",
  firmwareVersion: "2.0.6",
  bootloaderVersion: "2.5",
};

const LISTING: DeviceListing = {
  id: "usb:1:4",
  label: "KM003C — SERIAL-0004",
  model: "KM003C",
  serial: "SERIAL-0004",
  bus: "1",
  address: 4,
  demo_scenario: null,
  mode: "application",
};

const DETAILS: ConnectedInfo = {
  model: "KM003C",
  firmware_version: "2.0.6",
  bootloader_version: "2.5",
  mode: "application",
  warnings: ["outdated"],
  update_required: false,
  firmware_problems: [],
  serial: "SERIAL-0004",
  rate_sps: 50,
  rate_locked: false,
  session_id: 1,
  min_supported_firmware: "1.7.0",
  known_latest_firmware: "2.0.7",
};

/** A controllable flash: the test reports stages and settles it. */
function deferredFlash() {
  let onStage: (s: FlashEvent) => void = () => {};
  let resolve: (d: FlashDone) => void = () => {};
  let reject: (e: UiError) => void = () => {};
  const flash = vi.fn(
    (_id: string, _path: string, _confirmation: string, stage: (s: FlashEvent) => void) => {
      onStage = stage;
      return new Promise<FlashDone>((res, rej) => {
        resolve = res;
        reject = rej;
      });
    },
  );
  return {
    flash,
    stage: (s: FlashEvent) => onStage(s),
    resolve: (d: FlashDone) => resolve(d),
    reject: (e: UiError) => reject(e),
  };
}

function api(overrides: Partial<FirmwareUpdateApi> = {}): FirmwareUpdateApi {
  return {
    choose: vi.fn(async () => "/fw/KM003C_V2.0.7.mencrypt"),
    inspect: vi.fn(async () => INFO),
    flash: vi.fn(async () => ({ outcome: "verified", version: "2.0.7" }) as FlashDone),
    ...overrides,
  };
}

async function withFile(a: FirmwareUpdateApi = api()): Promise<FirmwareUpdate> {
  const fw = new FirmwareUpdate(a);
  await fw.chooseFile();
  return fw;
}

/** Let pending promise callbacks run. */
async function settle(): Promise<void> {
  for (let i = 0; i < 5; i++) await Promise.resolve();
}

describe("the meter (AC-V9SK55.2)", () => {
  it("takes the picked device's mode and, while connected, both versions", () => {
    expect(flashMeter(LISTING, DETAILS)).toEqual(METER);
    expect(flashMeter(LISTING, null)).toEqual({
      ...METER,
      firmwareVersion: null,
      bootloaderVersion: null,
    });
    expect(flashMeter({ ...LISTING, mode: "bootloader" }, null)?.mode).toBe("bootloader");
    expect(flashMeter(undefined, null)).toBeNull();
  });

  it("shows mode, firmware and bootloader version, with dashes while unknown", () => {
    expect(firmwareMeterRows(METER)).toEqual([
      { label: "Model", value: "KM003C" },
      { label: "Mode", value: "Application" },
      { label: "Firmware", value: "2.0.6" },
      { label: "Bootloader", value: "2.5" },
    ]);
    const boot = firmwareMeterRows({
      ...METER,
      mode: "bootloader",
      firmwareVersion: null,
      bootloaderVersion: null,
    });
    expect(boot.find((r) => r.label === "Mode")?.value).toBe("Bootloader");
    expect(boot.find((r) => r.label === "Firmware")?.value).toBe("—");
    expect(boot.find((r) => r.label === "Bootloader")?.value).toBe("—");
  });
});

describe("choosing the file (AC-V9SK55.2)", () => {
  it("inspects the chosen file and keeps its metadata", async () => {
    const a = api();
    const fw = await withFile(a);
    expect(a.inspect).toHaveBeenCalledWith("/fw/KM003C_V2.0.7.mencrypt");
    expect(fw.path).toBe("/fw/KM003C_V2.0.7.mencrypt");
    expect(fw.info).toEqual(INFO);
    expect(fw.fileError).toBeNull();
  });

  it("a cancelled dialog changes nothing", async () => {
    const empty = api({ choose: vi.fn(async () => null) });
    const none = await withFile(empty);
    expect(none.info).toBeNull();
    expect(none.path).toBeNull();
    expect(empty.inspect).not.toHaveBeenCalled();
    // A file already chosen stays when a later dialog is cancelled.
    const choose = vi
      .fn<() => Promise<string | null>>()
      .mockResolvedValueOnce("/fw/KM003C_V2.0.7.mencrypt")
      .mockResolvedValueOnce(null);
    const fw = await withFile(api({ choose }));
    await fw.chooseFile();
    expect(fw.path).toBe("/fw/KM003C_V2.0.7.mencrypt");
    expect(fw.info).toEqual(INFO);
  });

  it("a file that fails inspection shows the reason and has no metadata", async () => {
    const err: UiError = { kind: "usage", message: "This is not a firmware file.", hint: null };
    const fw = await withFile(api({ inspect: vi.fn(async () => Promise.reject(err)) }));
    expect(fw.info).toBeNull();
    expect(fw.fileError).toBe("This is not a firmware file.");
    expect(fw.canUpdate(METER)).toBe(false);
  });
});

describe("Update is enabled only for an inspected file of the meter's model (AC-V9SK55.2)", () => {
  it("is disabled with no file", () => {
    const fw = new FirmwareUpdate(api());
    expect(fw.canUpdate(METER)).toBe(false);
    expect(fw.updateBlocker(METER)).toMatch(/file/i);
  });

  it("is disabled with no meter", async () => {
    const fw = await withFile();
    expect(fw.canUpdate(null)).toBe(false);
    expect(fw.updateBlocker(null)).toMatch(/meter/i);
  });

  it("is disabled when the file is for another model, naming both", async () => {
    const fw = await withFile(api({ inspect: vi.fn(async () => ({ ...INFO, model: "KM002C" })) }));
    expect(fw.canUpdate(METER)).toBe(false);
    const why = fw.updateBlocker(METER) ?? "";
    expect(why).toContain("KM002C");
    expect(why).toContain("KM003C");
  });

  it("is enabled for a matching model, in any letter case", async () => {
    const fw = await withFile(api({ inspect: vi.fn(async () => ({ ...INFO, model: "km003c" })) }));
    expect(fw.canUpdate(METER)).toBe(true);
    expect(fw.updateBlocker(METER)).toBeNull();
    expect(fw.canUpdate({ ...METER, mode: "bootloader", firmwareVersion: null })).toBe(true);
  });
});

describe("the typed confirmation (AC-V9SK55.3)", () => {
  it("warns to keep the meter connected and the laptop on power", () => {
    expect(UPDATE_WARNING).toMatch(/connected/i);
    expect(UPDATE_WARNING).toMatch(/laptop/i);
    expect(UPDATE_WARNING).toMatch(/power/i);
    // The only unplugging is the replug the screen asks for (AC-V9SK55.4).
    expect(UPDATE_WARNING).toMatch(/only when the screen asks/i);
  });

  it("Update opens the confirmation; it does not flash yet", async () => {
    const a = api();
    const fw = await withFile(a);
    fw.requestUpdate(METER);
    expect(fw.phase).toBe("confirming");
    expect(fw.confirmation).toBe("");
    expect(a.flash).not.toHaveBeenCalled();
  });

  it("a blocked Update does not open the confirmation", async () => {
    const fw = await withFile(api({ inspect: vi.fn(async () => ({ ...INFO, model: "KM002C" })) }));
    fw.requestUpdate(METER);
    expect(fw.phase).toBe("idle");
  });

  it("starts only when the typed text is the target version", async () => {
    const a = api();
    const fw = await withFile(a);
    fw.requestUpdate(METER);
    for (const typed of ["", "2.0.6", "2.0.7x", "2.0", "yes"]) {
      fw.confirmation = typed;
      expect(fw.confirmed).toBe(false);
      await fw.start(METER);
      expect(a.flash).not.toHaveBeenCalled();
      expect(fw.phase).toBe("confirming");
    }
    fw.confirmation = "2.0.7";
    expect(fw.confirmed).toBe(true);
    await fw.start(METER);
    expect(a.flash).toHaveBeenCalledTimes(1);
    const call = vi.mocked(a.flash).mock.calls[0]!;
    expect(call.slice(0, 3)).toEqual(["usb:1:4", "/fw/KM003C_V2.0.7.mencrypt", "2.0.7"]);
  });

  it("Cancel closes the confirmation, clears the text and sends nothing", async () => {
    const a = api();
    const fw = await withFile(a);
    fw.requestUpdate(METER);
    fw.confirmation = "2.0.7";
    fw.cancel();
    expect(fw.phase).toBe("idle");
    expect(fw.confirmation).toBe("");
    expect(a.flash).not.toHaveBeenCalled();
  });
});

describe("progress (AC-V9SK55.4)", () => {
  it("lists the stages in flash order", () => {
    expect(FLASH_STAGES).toEqual([
      "switching",
      "erasing",
      "writing",
      "verifying",
      "finished",
      "returning",
    ]);
  });

  it("names each stage; the write stage shows bytes done / total", () => {
    const labels = FLASH_STAGES.map((name) =>
      stageLabel(name === "writing" ? { stage: name, done: 0, total: 1 } : { stage: name }),
    );
    expect(new Set(labels).size).toBe(FLASH_STAGES.length);
    expect(stageLabel({ stage: "switching" })).toMatch(/bootloader/i);
    expect(stageLabel({ stage: "erasing" })).toMatch(/eras/i);
    expect(stageLabel({ stage: "verifying" })).toMatch(/verif/i);
    expect(stageLabel({ stage: "returning" })).toMatch(/application/i);
    const w = stageLabel({ stage: "writing", done: 4080, total: 12340 });
    expect(w).toMatch(/writ/i);
    expect(w).toContain("4,080");
    expect(w).toContain("12,340");
  });

  it("follows each stage while busy, with the write progress", async () => {
    const d = deferredFlash();
    const fw = await withFile(api({ flash: d.flash }));
    fw.requestUpdate(METER);
    fw.confirmation = "2.0.7";
    const running = fw.start(METER);
    await settle();
    expect(fw.phase).toBe("updating");
    expect(fw.busy).toBe(true);
    d.stage({ stage: "switching" });
    expect(fw.stage).toEqual({ stage: "switching" });
    expect(fw.writeProgress).toBeNull();
    d.stage({ stage: "erasing" });
    d.stage({ stage: "writing", done: 4080, total: 12340 });
    expect(fw.stage?.stage).toBe("writing");
    expect(fw.writeProgress).toEqual({ done: 4080, total: 12340 });
    d.stage({ stage: "writing", done: 12340, total: 12340 });
    d.stage({ stage: "verifying" });
    expect(fw.writeProgress).toEqual({ done: 12340, total: 12340 });
    d.resolve({ outcome: "verified", version: "2.0.7" });
    await running;
    expect(fw.busy).toBe(false);
  });
});

describe("the outcome (AC-V9SK55.5)", () => {
  it("success shows the version read back from the meter", async () => {
    const fw = await withFile(
      api({ flash: vi.fn(async () => ({ outcome: "verified", version: "2.0.7" }) as FlashDone) }),
    );
    fw.requestUpdate(METER);
    fw.confirmation = "2.0.7";
    await fw.start(METER);
    expect(fw.phase).toBe("done");
    expect(fw.result).toEqual({ outcome: "verified", version: "2.0.7" });
    expect(fw.error).toBeNull();
  });

  it("an image accepted while the meter stays in its bootloader is done, asking for a replug", async () => {
    const fw = await withFile(
      api({ flash: vi.fn(async () => ({ outcome: "power_cycle_needed" }) as FlashDone) }),
    );
    fw.requestUpdate(METER);
    fw.confirmation = "2.0.7";
    await fw.start(METER);
    expect(fw.phase).toBe("done");
    expect(fw.result).toEqual({ outcome: "power_cycle_needed" });
  });

  it("failure keeps the error and the stage it failed at", async () => {
    const d = deferredFlash();
    const fw = await withFile(api({ flash: d.flash }));
    fw.requestUpdate(METER);
    fw.confirmation = "2.0.7";
    const running = fw.start(METER);
    await settle();
    d.stage({ stage: "switching" });
    d.stage({ stage: "erasing" });
    const err: UiError = {
      kind: "protocol",
      message: "erase error: no ACCEPT after the erase",
      hint: "Hold Back while plugging the meter in, then run the update again.",
    };
    d.reject(err);
    await running;
    expect(fw.phase).toBe("failed");
    expect(fw.error).toEqual(err);
    expect(fw.failedStage).toBe("erasing");
    expect(fw.busy).toBe(false);
  });

  it("the recovery steps say to hold Back while plugging in, then retry", () => {
    const text = RECOVERY_STEPS.join(" ");
    expect(RECOVERY_STEPS.length).toBeGreaterThanOrEqual(2);
    expect(text).toContain("Back");
    expect(text).toMatch(/plug/i);
    expect(text).toMatch(/again|retry/i);
  });

  it("dismissing the outcome goes back to the file, ready for another try", async () => {
    const fw = await withFile(
      api({
        flash: vi.fn(async () => Promise.reject({ kind: "timeout", message: "x", hint: null })),
      }),
    );
    fw.requestUpdate(METER);
    fw.confirmation = "2.0.7";
    await fw.start(METER);
    expect(fw.phase).toBe("failed");
    fw.dismiss();
    expect(fw.phase).toBe("idle");
    expect(fw.error).toBeNull();
    expect(fw.info).toEqual(INFO);
    expect(fw.canUpdate(METER)).toBe(true);
  });
});

describe("review fixes (AC-V9SK55.4, .5)", () => {
  it("choosing a file is ignored while confirming or updating", async () => {
    const d = deferredFlash();
    const a = api({ flash: d.flash });
    const fw = await withFile(a);
    fw.requestUpdate(METER);
    await fw.chooseFile();
    expect(a.choose).toHaveBeenCalledTimes(1);
    fw.confirmation = "2.0.7";
    const running = fw.start(METER);
    await settle();
    await fw.chooseFile();
    expect(a.choose).toHaveBeenCalledTimes(1);
    d.resolve({ outcome: "verified", version: "2.0.7" });
    await running;
  });

  it("a usage refusal before any stage is not a recovery case", async () => {
    const fw = await withFile(
      api({
        flash: vi.fn(async () =>
          Promise.reject({ kind: "usage", message: "Refused.", hint: null } satisfies UiError),
        ),
      }),
    );
    fw.requestUpdate(METER);
    fw.confirmation = "2.0.7";
    await fw.start(METER);
    expect(fw.phase).toBe("failed");
    expect(fw.needsRecovery).toBe(false);
  });

  it("a failure after a stage was reached needs the recovery steps", async () => {
    const d = deferredFlash();
    const fw = await withFile(api({ flash: d.flash }));
    fw.requestUpdate(METER);
    fw.confirmation = "2.0.7";
    const running = fw.start(METER);
    await settle();
    d.stage({ stage: "erasing" });
    d.reject({ kind: "protocol", message: "x", hint: null });
    await running;
    expect(fw.needsRecovery).toBe(true);
  });

  it("a usage error is a refusal even when a stage was reported", async () => {
    const d = deferredFlash();
    const fw = await withFile(api({ flash: d.flash }));
    fw.requestUpdate(METER);
    fw.confirmation = "2.0.7";
    const running = fw.start(METER);
    await settle();
    d.stage({ stage: "switching" });
    d.reject({ kind: "usage", message: "x", hint: null });
    await running;
    expect(fw.needsRecovery).toBe(false);
  });

  it("the meter's versions stay shown while the update runs, then follow the meter again", async () => {
    const d = deferredFlash();
    const fw = await withFile(api({ flash: d.flash }));
    const disconnected = { ...METER, firmwareVersion: null, bootloaderVersion: null };
    expect(fw.meterShown(METER)).toEqual(METER);
    fw.requestUpdate(METER);
    fw.confirmation = "2.0.7";
    const running = fw.start(METER);
    await settle();
    // The update disconnects the meter, so its versions are no longer read.
    expect(fw.meterShown(disconnected)).toEqual(METER);
    d.resolve({ outcome: "verified", version: "2.0.7" });
    await running;
    // Done: the old versions are stale now, so show the meter as it is.
    expect(fw.meterShown(disconnected)).toEqual(disconnected);
    expect(fw.meterShown(null)).toBeNull();
  });

  it("the write stage names no byte counts before the write starts", () => {
    expect(stageNameLabel("writing", null)).toBe("Writing the image");
    expect(stageNameLabel("writing", { done: 4080, total: 12340 })).toBe(
      "Writing 4,080 / 12,340 bytes",
    );
    expect(stageNameLabel("erasing", null)).toBe("Erasing the application area");
  });
});

describe("the replug prompt (AC-V9SK55.4, AC-V9SK54.1 / .5)", () => {
  it("a replug report shows the prompt without leaving the current stage", async () => {
    const d = deferredFlash();
    const fw = await withFile(api({ flash: d.flash }));
    fw.requestUpdate(METER);
    fw.confirmation = "2.0.7";
    const running = fw.start(METER);
    await settle();
    expect(fw.replug).toBeNull();
    d.stage({ stage: "switching" });
    d.stage({ stage: "replug", to: "bootloader", reply: "none" });
    expect(fw.stage).toEqual({ stage: "switching" });
    expect(fw.replug).toEqual({ to: "bootloader", reply: "none" });
    d.stage({ stage: "erasing" });
    expect(fw.replug).toBeNull();
    d.stage({ stage: "returning" });
    d.stage({ stage: "replug", to: "application", reply: "none" });
    expect(fw.stage).toEqual({ stage: "returning" });
    expect(fw.replug?.to).toBe("application");
    d.resolve({ outcome: "verified", version: "2.0.7" });
    await running;
    expect(fw.replug).toBeNull();
  });

  it("a failure while waiting for the replug clears the prompt and keeps the stage", async () => {
    const d = deferredFlash();
    const fw = await withFile(api({ flash: d.flash }));
    fw.requestUpdate(METER);
    fw.confirmation = "2.0.7";
    const running = fw.start(METER);
    await settle();
    d.stage({ stage: "switching" });
    d.stage({ stage: "replug", to: "bootloader", reply: "none" });
    d.reject({ kind: "protocol", message: "not in bootloader mode", hint: null });
    await running;
    expect(fw.replug).toBeNull();
    expect(fw.failedStage).toBe("switching");
  });

  it("tells the user to unplug and replug, naming the mode it waits for", () => {
    const boot = replugPrompt({ to: "bootloader", reply: "none" });
    expect(boot).toMatch(/unplug the meter and plug it back in/i);
    expect(boot).toContain("bootloader");
    expect(replugPrompt({ to: "application", reply: "none" })).toContain("application");
  });
});
