/**
 * FR AXHK2F: an update starts only once the user agreed to every session it would end.
 * The backend decides: it refuses with `flash_needs_consent` and the page opens the
 * prompt. All on a fake backend; nothing reaches a meter.
 */

import { describe, expect, it, vi } from "vitest";
import type { FirmwareFileInfo, FlashDone, FlashHold, RecordingSummary, UiError } from "./backend";
import {
  FirmwareUpdate,
  type FirmwareMeter,
  type FirmwareUpdateApi,
} from "./firmwareUpdate.svelte";
import { Recording, type RecordingDeps } from "./recording.svelte";

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
  id: "usb:1:5",
  model: "KM003C",
  mode: "application",
  firmwareVersion: "2.0.6",
  bootloaderVersion: "2.5",
};

const RECORDING: FlashHold = {
  id: "usb:1:4",
  label: "KM003C — bus 1 address 4",
  recording: true,
};
const IDLE: FlashHold = { id: "usb:1:5", label: "KM003C — bus 1 address 5", recording: false };

function refusal(sessions: FlashHold[]): UiError {
  return {
    kind: "flash_needs_consent",
    message: "The update would end these sessions.",
    hint: null,
    sessions,
  } as UiError;
}

/** A backend that refuses each flash with the next of `refusals`, then lets the update
 * run; it stays running until the test ends. */
function backend(refusals: (FlashHold[] | UiError)[], over: Partial<FirmwareUpdateApi> = {}) {
  const queue = [...refusals];
  const flash = vi.fn<FirmwareUpdateApi["flash"]>(() => {
    const next = queue.shift();
    if (next === undefined) return new Promise<FlashDone>(() => {});
    return Promise.reject(Array.isArray(next) ? refusal(next) : next);
  });
  const stopRecording = vi.fn<NonNullable<FirmwareUpdateApi["stopRecording"]>>(async () => {});
  const api = {
    choose: vi.fn(async () => "/fw/KM003C_V2.0.7.mencrypt"),
    inspect: vi.fn(async () => INFO),
    flash,
    cancelFlash: vi.fn(async () => {}),
    stopRecording,
    ...over,
  } as FirmwareUpdateApi;
  return { api, flash, stopRecording };
}

async function confirming(api: FirmwareUpdateApi): Promise<FirmwareUpdate> {
  const fw = new FirmwareUpdate(api);
  await fw.chooseFile();
  fw.requestUpdate(METER);
  fw.confirmation = "2.0.7";
  return fw;
}

async function settle(): Promise<void> {
  for (let i = 0; i < 10; i++) await Promise.resolve();
}

describe("the prompt (AC-AXHK2F.4)", () => {
  it("opens from the backend's refusal, with every session it named", async () => {
    const b = backend([[RECORDING, IDLE]]);
    const fw = await confirming(b.api);
    await fw.start(METER);

    expect(fw.releasePrompt).toEqual([RECORDING, IDLE]);
    expect(b.flash).toHaveBeenCalledOnce();
    expect(b.flash.mock.calls[0]?.[4] ?? []).toEqual([]);
    expect(b.stopRecording).not.toHaveBeenCalled();
  });

  it("is not a failure: the confirmation stays open and nothing shows as updating", async () => {
    const b = backend([[IDLE]]);
    const fw = await confirming(b.api);
    await fw.start(METER);

    expect(fw.phase).toBe("confirming");
    expect(fw.confirmation).toBe("2.0.7");
    expect(fw.error).toBeNull();
    expect(fw.busy).toBe(false);
    expect(fw.updatingId).toBeNull();
  });

  it("declining ends nothing and starts nothing, and goes back to the confirmation", async () => {
    const b = backend([[RECORDING, IDLE]]);
    const fw = await confirming(b.api);
    await fw.start(METER);

    fw.declineRelease();

    expect(fw.releasePrompt).toBeNull();
    expect(fw.phase).toBe("confirming");
    expect(fw.confirmation).toBe("2.0.7");
    expect(b.stopRecording).not.toHaveBeenCalled();
    expect(b.flash).toHaveBeenCalledOnce();
  });

  it("a consent that was declined is not sent the next time", async () => {
    const b = backend([[IDLE], [IDLE]]);
    const fw = await confirming(b.api);
    await fw.start(METER);
    fw.declineRelease();

    await fw.start(METER);

    expect(b.flash).toHaveBeenCalledTimes(2);
    expect(b.flash.mock.calls[1]?.[4] ?? []).toEqual([]);
    expect(fw.releasePrompt).toEqual([IDLE]);
  });

  it("agreeing ends each recording, then starts the update naming every session", async () => {
    const b = backend([[RECORDING, IDLE]]);
    const fw = await confirming(b.api);
    await fw.start(METER);

    void fw.confirmRelease(METER);
    await settle();

    expect(b.stopRecording.mock.calls).toEqual([[RECORDING.id]]);
    expect(b.flash).toHaveBeenCalledTimes(2);
    expect(b.flash.mock.calls[1]?.[4]).toEqual([RECORDING, IDLE]);
    expect(b.stopRecording.mock.invocationCallOrder[0]).toBeLessThan(
      b.flash.mock.invocationCallOrder[1] ?? 0,
    );
    expect(fw.releasePrompt).toBeNull();
    expect(fw.phase).toBe("updating");
  });

  it("does not open for any other refusal", async () => {
    const other: UiError = { kind: "usage", message: "That meter is gone.", hint: null };
    const b = backend([other]);
    const fw = await confirming(b.api);
    await fw.start(METER);

    expect(fw.releasePrompt).toBeNull();
    expect(fw.phase).toBe("failed");
    expect(fw.error).toEqual(other);
  });
});

describe("a recording that starts after the consent (AC-AXHK2F.5)", () => {
  it("reopens the prompt with what the backend named, and the update stays unstarted", async () => {
    const started: FlashHold = { ...IDLE, recording: true };
    const b = backend([[IDLE], [started]]);
    const fw = await confirming(b.api);
    await fw.start(METER);

    await fw.confirmRelease(METER);

    expect(fw.releasePrompt).toEqual([started]);
    expect(fw.phase).toBe("confirming");
    expect(fw.error).toBeNull();
    expect(fw.busy).toBe(false);
    expect(b.stopRecording).not.toHaveBeenCalled();
  });

  it("needs its own answer", async () => {
    const started: FlashHold = { ...IDLE, recording: true };
    const b = backend([[IDLE], [started]]);
    const fw = await confirming(b.api);
    await fw.start(METER);
    await fw.confirmRelease(METER);

    void fw.confirmRelease(METER);
    await settle();

    expect(b.stopRecording.mock.calls).toEqual([[started.id]]);
    expect(b.flash).toHaveBeenCalledTimes(3);
    expect(b.flash.mock.calls[2]?.[4]).toEqual([started]);
    expect(fw.phase).toBe("updating");
  });
});

describe("a stop that fails (AC-AXHK2F.7)", () => {
  it("leaves the update unstarted, with the reason", async () => {
    const reason: UiError = { kind: "internal", message: "The file did not close.", hint: null };
    const b = backend([[RECORDING]], { stopRecording: vi.fn(() => Promise.reject(reason)) });
    const fw = await confirming(b.api);
    await fw.start(METER);

    await fw.confirmRelease(METER);

    expect(b.flash).toHaveBeenCalledOnce();
    expect(fw.error).toEqual(reason);
    expect(fw.needsRecovery).toBe(false);
    expect(fw.releasePrompt).toBeNull();
  });

  it("stops before the next recording is touched", async () => {
    const second: FlashHold = { id: "usb:1:6", label: "KM003C — bus 1 address 6", recording: true };
    const stopRecording = vi.fn(() => Promise.reject(new Error("no")));
    const b = backend([[RECORDING, second]], { stopRecording });
    const fw = await confirming(b.api);
    await fw.start(METER);

    await fw.confirmRelease(METER);

    expect(stopRecording).toHaveBeenCalledOnce();
    expect(b.flash).toHaveBeenCalledOnce();
  });
});

describe("a stop that cannot happen yet (AC-AXHK2F.7)", () => {
  const never = (): Promise<never> => new Promise(() => {});
  const recordingDeps = (over: Partial<RecordingDeps> = {}): RecordingDeps => ({
    canRecord: true,
    startRecording: vi.fn(async () => "/tmp/rec.csv"),
    stopRecording: vi.fn<() => Promise<RecordingSummary>>(never),
    now: () => new Date(2026, 9, 21, 9, 5, 7),
    ...over,
  });

  /** The app's `stopRecording`: a false answer throws what the recording reports. */
  function stoppingVia(recording: Recording, d: RecordingDeps): Partial<FirmwareUpdateApi> {
    return {
      stopRecording: async () => {
        if (!(await recording.stop(d))) throw recording.error;
      },
    };
  }

  async function refusedWhile(recording: Recording, d: RecordingDeps): Promise<FirmwareUpdate> {
    const b = backend([[RECORDING]], stoppingVia(recording, d));
    const fw = await confirming(b.api);
    await fw.start(METER);

    await fw.confirmRelease(METER);

    expect(b.flash).toHaveBeenCalledOnce();
    expect(fw.busy).toBe(false);
    expect(fw.updatingId).toBeNull();
    expect(fw.releasePrompt).toBeNull();
    expect(fw.needsRecovery).toBe(false);
    return fw;
  }

  function expectReason(fw: FirmwareUpdate): void {
    const message = fw.error?.message ?? "";
    expect(message).not.toBe("");
    expect(message).not.toMatch(/null|undefined/);
    expect(message).toMatch(/record/i);
    expect(message).toMatch(/start|stop/i);
    expect(message).toMatch(/\.$/);
  }

  it("while a start is in flight, says so in a sentence", async () => {
    const recording = new Recording();
    const d = recordingDeps({ startRecording: vi.fn(never) });
    void recording.toggle(d);
    expect(recording.phase).toBe("starting");

    expectReason(await refusedWhile(recording, d));
  });

  it("while a stop is in flight, says so in a sentence", async () => {
    const recording = new Recording();
    const d = recordingDeps();
    await recording.toggle(d);
    void recording.stop(d);
    expect(recording.phase).toBe("stopping");

    expectReason(await refusedWhile(recording, d));
  });
});
