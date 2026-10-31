/**
 * FR 4PJRVJ: the firmware view can cancel an update until the erase starts (AC .1), asks
 * before ending a running recording (AC .2) and picks a running update up after a page
 * reload (AC .3). All on a fake backend; nothing reaches a meter.
 */

import { afterEach, describe, expect, it, vi } from "vitest";
import type { FirmwareFileInfo, FlashDone, FlashEvent, UiError } from "./backend";
import {
  CANCEL_PENDING,
  FirmwareUpdate,
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

/** A flash the test drives; the fake backend's other calls are spies. */
function fakeBackend(overrides: Partial<FirmwareUpdateApi> = {}) {
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
    isRecording: vi.fn(() => false),
    stopRecording: vi.fn(async () => {}),
    ...overrides,
  } as FirmwareUpdateApi;
  return {
    api,
    stage: (s: FlashEvent) => onStage(s),
    reject: (e: UiError) => reject(e),
    resolve: (d: FlashDone) => resolve(d),
  };
}

async function confirming(api: FirmwareUpdateApi): Promise<FirmwareUpdate> {
  const fw = new FirmwareUpdate(api);
  await fw.chooseFile();
  fw.requestUpdate(METER);
  fw.confirmation = "2.0.7";
  return fw;
}

async function settle(): Promise<void> {
  for (let i = 0; i < 5; i++) await Promise.resolve();
}

describe("cancel (AC-4PJRVJ.1)", () => {
  it("can be cancelled before the erase, and asks the backend to stop", async () => {
    const b = fakeBackend();
    const fw = await confirming(b.api);
    void fw.start(METER);
    await settle();
    expect(fw.cancelBlocker).toBeNull();
    b.stage({ stage: "switching" });
    expect(fw.cancelBlocker).toBeNull();

    await fw.cancelUpdate();
    expect(b.api.cancelFlash).toHaveBeenCalledOnce();

    b.reject({ kind: "usage", message: "The update was stopped.", hint: null });
    await settle();
    expect(fw.phase).not.toBe("updating");
  });

  it.each([
    [{ stage: "erasing" }],
    [{ stage: "writing", done: 1, total: 2 }],
    [{ stage: "verifying" }],
    [{ stage: "finished" }],
    [{ stage: "returning" }],
  ] satisfies [FlashEvent][])("is refused with a reason at %j", async (stage) => {
    const b = fakeBackend();
    const fw = await confirming(b.api);
    void fw.start(METER);
    await settle();
    b.stage(stage);
    expect(fw.cancelBlocker).toMatch(/eras/i);

    await fw.cancelUpdate();
    expect(b.api.cancelFlash).not.toHaveBeenCalled();
    expect(fw.phase).toBe("updating");
  });

  it("stays shut when a replug prompt follows the erase", async () => {
    const b = fakeBackend();
    const fw = await confirming(b.api);
    void fw.start(METER);
    await settle();
    b.stage({ stage: "returning" });
    b.stage({ stage: "replug", to: "application", reply: "none" });
    expect(fw.cancelBlocker).toMatch(/eras/i);
  });
});

describe("a running recording (AC-4PJRVJ.2)", () => {
  function recording() {
    return fakeBackend({ isRecording: vi.fn((id: string) => id === METER.id) });
  }

  it("asks first, and neither stops the recording nor starts the update", async () => {
    const b = recording();
    const fw = await confirming(b.api);
    await fw.start(METER);
    expect(fw.recordingPrompt).toBe(true);
    expect(b.api.flash).not.toHaveBeenCalled();
    expect(b.api.stopRecording).not.toHaveBeenCalled();
  });

  it("keeps recording when the user declines", async () => {
    const b = recording();
    const fw = await confirming(b.api);
    await fw.start(METER);
    fw.declineEndRecording();
    expect(fw.recordingPrompt).toBe(false);
    expect(fw.phase).toBe("confirming");
    expect(b.api.flash).not.toHaveBeenCalled();
    expect(b.api.stopRecording).not.toHaveBeenCalled();
  });

  it("ends the recording and then updates when the user agrees", async () => {
    const b = recording();
    const fw = await confirming(b.api);
    await fw.start(METER);
    void fw.confirmEndRecording(METER);
    await settle();
    expect(b.api.stopRecording).toHaveBeenCalledWith(METER.id);
    expect(b.api.flash).toHaveBeenCalledOnce();
    expect(fw.recordingPrompt).toBe(false);
    expect(fw.phase).toBe("updating");
  });

  it("does not ask for a recording on another meter", async () => {
    const b = fakeBackend({ isRecording: vi.fn((id: string) => id === "usb:1:9") });
    const fw = await confirming(b.api);
    void fw.start(METER);
    await settle();
    expect(fw.recordingPrompt).toBe(false);
    expect(b.api.flash).toHaveBeenCalledOnce();
    expect(b.api.stopRecording).not.toHaveBeenCalled();
  });
});

describe("a page reload (AC-4PJRVJ.3)", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it("shows the running update's stage and holds the meter", async () => {
    const b = fakeBackend({
      flashStatus: vi.fn(async () => ({
        device_id: METER.id,
        stage: { stage: "writing" as const, done: 4096, total: 8192 },
      })),
    });
    const fw = new FirmwareUpdate(b.api);
    await fw.reattach();
    expect(fw.phase).toBe("updating");
    expect(fw.busy).toBe(true);
    expect(fw.stage).toEqual({ stage: "writing", done: 4096, total: 8192 });
    expect(fw.writeProgress).toEqual({ done: 4096, total: 8192 });
  });

  it("holds the meter while the backend has no stage yet", async () => {
    const b = fakeBackend({
      flashStatus: vi.fn(async () => ({ device_id: METER.id, stage: null })),
    });
    const fw = new FirmwareUpdate(b.api);
    await fw.reattach();
    expect(fw.phase).toBe("updating");
    expect(fw.stage).toBeNull();
  });

  it("shows the updating meter's id, not the picked one", async () => {
    const b = fakeBackend({
      flashStatus: vi.fn(async () => ({ device_id: "usb:other", stage: null })),
    });
    const fw = new FirmwareUpdate(b.api);
    await fw.reattach();
    expect(fw.meterShown(METER)?.id).toBe("usb:other");
    expect(fw.meterShown(METER)?.model).not.toBe(METER.model);
  });

  it("stays idle when no update runs", async () => {
    const b = fakeBackend();
    const fw = new FirmwareUpdate(b.api);
    await fw.reattach();
    expect(b.api.flashStatus).toHaveBeenCalledOnce();
    expect(fw.phase).toBe("idle");
    expect(fw.busy).toBe(false);
  });

  it("lets go once the backend's update has ended", async () => {
    vi.useFakeTimers();
    let running = true;
    const b = fakeBackend({
      flashStatus: vi.fn(async () =>
        running ? { device_id: METER.id, stage: { stage: "verifying" as const } } : null,
      ),
    });
    const fw = new FirmwareUpdate(b.api);
    await fw.reattach();
    expect(fw.busy).toBe(true);
    running = false;
    await vi.advanceTimersByTimeAsync(60_000);
    expect(fw.busy).toBe(false);
    expect(fw.followedEnded).toBe(true);
    expect(fw.followedLost).toBe(false);
  });

  it("stops following a backend that no longer answers", async () => {
    vi.useFakeTimers();
    let answers = true;
    const b = fakeBackend({
      flashStatus: vi.fn(async () => {
        if (!answers) throw new Error("gone");
        return { device_id: METER.id, stage: null };
      }),
    });
    const fw = new FirmwareUpdate(b.api);
    await fw.reattach();
    answers = false;
    await vi.advanceTimersByTimeAsync(60_000);
    expect(fw.busy).toBe(false);
    expect(fw.followedLost).toBe(true);
    expect(fw.followedEnded).toBe(false);
  });
});

describe("an accepted Cancel (AC-4PJRVJ.1)", () => {
  it("turns Cancel off and says what is happening", async () => {
    const b = fakeBackend({
      flashStatus: vi.fn(async () => ({
        device_id: METER.id,
        stage: { stage: "switching" as const },
      })),
    });
    const fw = new FirmwareUpdate(b.api);
    await fw.reattach();
    expect(fw.cancelBlocker).toBeNull();
    await fw.cancelUpdate();
    expect(fw.cancelling).toBe(true);
    expect(fw.cancelBlocker).toBe(CANCEL_PENDING);
  });
});
