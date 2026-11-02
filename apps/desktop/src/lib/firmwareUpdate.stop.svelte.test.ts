/**
 * FR AXHK2M, the reopened ACs from the a5442bc hardware run:
 * - .13: a stop the user asked for shows as stopped, not as a failure;
 * - .14: its next step is a plain replug first, the button only if that does not help,
 *   and a meter in bootloader mode is listed as such;
 * - .15: once the update ends (stopped or done) the page connects again to the same
 *   meter, by its serial, once it is back in application mode;
 * - .16: the Firmware tab shows the meter's versions after an update, never a dash.
 * All on fake backends; nothing reaches a meter.
 */

import type { Component } from "svelte";
import { render } from "svelte/server";
import { describe, expect, it, vi } from "vitest";
import type {
  ConnectedInfo,
  DeviceListing,
  FirmwareFileInfo,
  FlashDone,
  FlashEvent,
  FlashStatus,
  UiError,
} from "./backend";
import { toUiError } from "./backend";
import FirmwareView from "./components/FirmwareView.svelte";
import { deviceOptionLabel } from "./format";
import {
  FirmwareUpdate,
  firmwareMeterRows,
  flashMeter,
  type FirmwareMeter,
  type FirmwareUpdateApi,
} from "./firmwareUpdate.svelte";

const SERIAL = "SERIAL-FW-STOP-1";

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

/** The meter as the OS lists it at `address`, in `mode`. */
function listing(address: number, mode: DeviceListing["mode"] = "application"): DeviceListing {
  return {
    id: `usb:1:${address}`,
    label: `KM003C — ${SERIAL}`,
    model: "KM003C",
    serial: SERIAL,
    bus: "1",
    address,
    demo_scenario: null,
    mode,
  };
}

const DEMO: DeviceListing = {
  id: "demo:load-5v-1a",
  label: "Demo device — load-5v-1a",
  model: "KM003C",
  serial: "123456",
  bus: null,
  address: null,
  demo_scenario: "load-5v-1a",
  mode: "application",
};

const OTHER: DeviceListing = {
  ...listing(12),
  id: "usb:1:12",
  label: "KM003C — SERIAL-OTHER",
  serial: "SERIAL-OTHER",
};

function details(firmware: string, bootloader: string): ConnectedInfo {
  return {
    model: "KM003C",
    firmware_version: firmware,
    bootloader_version: bootloader,
    mode: "application",
    warnings: [],
  } as unknown as ConnectedInfo;
}

/** The meter at `usb:1:4`, connected, before the update. */
const METER: FirmwareMeter = {
  ...flashMeter(listing(4), details("2.0.6", "2.5"))!,
};

/** What the backend says when the user's stop ended the update (AC-AXHK2M.13). */
const STOPPED: UiError = {
  kind: "cancelled" as UiError["kind"],
  message:
    "The update was stopped before anything was erased. The meter may be waiting in its bootloader.",
  hint: null,
};

/** A flash the test drives; the fake backend's other calls are spies. */
function fakeBackend(overrides: Partial<FirmwareUpdateApi> = {}) {
  let onStage: (s: FlashEvent) => void = () => {};
  let reject: (e: UiError) => void = () => {};
  let resolve: (d: FlashDone) => void = () => {};
  const reconnect = vi.fn<(id: string) => Promise<void>>(async () => {});
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
    reconnect,
    ...overrides,
  } as unknown as FirmwareUpdateApi;
  return {
    api,
    reconnect,
    stage: (s: FlashEvent) => onStage(s),
    reject: (e: UiError) => reject(e),
    resolve: (d: FlashDone) => resolve(d),
  };
}

async function settle(): Promise<void> {
  for (let i = 0; i < 8; i++) await Promise.resolve();
}

/** An update of `meter` started and running at the switch. */
async function running(b: ReturnType<typeof fakeBackend>, meter: FirmwareMeter = METER) {
  const fw = new FirmwareUpdate(b.api);
  await fw.chooseFile();
  fw.requestUpdate(meter);
  fw.confirmation = "2.0.7";
  void fw.start(meter);
  await settle();
  b.stage({ stage: "switching" });
  return fw;
}

/** An update the user stopped during the switch, as the backend reports it. */
async function stopped(b = fakeBackend()): Promise<FirmwareUpdate> {
  const fw = await running(b);
  b.stage({ stage: "replug", to: "bootloader", reply: "none" });
  await fw.cancelUpdate();
  b.reject(STOPPED);
  await settle();
  return fw;
}

/** An update that ran to its end and read the new version back. */
async function done(b = fakeBackend(), meter: FirmwareMeter = METER): Promise<FirmwareUpdate> {
  const fw = await running(b, meter);
  for (const stage of ["erasing", "verifying", "finished", "returning"] as const) {
    b.stage({ stage });
  }
  b.resolve({ outcome: "verified", version: "2.0.7" });
  await settle();
  return fw;
}

/** The HTML of `component` with `props`, without Svelte's comment markers. */
function html(component: unknown, props: Record<string, unknown>): string {
  const body = render(component as Component<Record<string, unknown>>, { props }).body;
  return body.replace(/<!--.*?-->/g, "");
}

function text(markup: string): string {
  return markup
    .replace(/<[^>]*>/g, " ")
    .replace(/\s+/g, " ")
    .trim();
}

/** The text of each `<li>` after the progress section (the outcome's steps), in order;
 * the release notes and the stage list come before it. */
function items(markup: string): string[] {
  const outcome = markup.slice(markup.lastIndexOf("</section>"));
  return [...outcome.matchAll(/<li\b[^>]*>(.*?)<\/li>/gs)].map((m) => text(m[1] ?? ""));
}

function view(fw: FirmwareUpdate): string {
  return html(FirmwareView, { firmware: fw, meter: METER, hideSerial: false });
}

// ---------------------------------------------------------------- AC-AXHK2M.13

describe("a stop the user asked for (AC-AXHK2M.13)", () => {
  it("keeps the backend's `cancelled` kind through the invoke error mapping", () => {
    expect(toUiError({ ...STOPPED })).toEqual(STOPPED);
  });

  it("ends as stopped, not as failed, with no recovery steps", async () => {
    const fw = await stopped();
    expect(fw.phase).toBe("stopped");
    expect(fw.busy).toBe(false);
    expect(fw.needsRecovery).toBe(false);
  });

  it("ends as stopped when a reloaded page reads the kept stop", async () => {
    const kept: FlashStatus = {
      device_id: METER.id,
      stage: { stage: "switching" },
      replug: null,
      outcome: { result: null, error: STOPPED },
    };
    const b = fakeBackend({ flashStatus: vi.fn(async () => kept) });
    const fw = new FirmwareUpdate(b.api);
    await fw.reattach();
    await settle();
    expect(fw.phase).toBe("stopped");
    expect(fw.needsRecovery).toBe(false);
  });

  it("a real failure still ends as failed", async () => {
    const b = fakeBackend();
    const fw = await running(b);
    b.reject({
      kind: "protocol",
      message: "The firmware update failed: verify error.",
      hint: null,
    });
    await settle();
    expect(fw.phase).toBe("failed");
  });

  it("shows no error colour, no alert and no failed stage", async () => {
    const markup = view(await stopped());
    expect(text(markup)).toMatch(/stopped/i);
    expect(markup).not.toMatch(/role="alert"/);
    expect(markup).not.toMatch(/class="[^"]*\berror\b[^"]*"/);
    expect(markup).not.toMatch(/data-state="failed"/);
  });

  it("offers no retry and does not call it a failure", async () => {
    const shown = text(view(await stopped()));
    expect(shown).not.toMatch(/retry the update/i);
    expect(shown).not.toMatch(/run the update again/i);
    expect(shown).not.toMatch(/failed/i);
    expect(shown).not.toMatch(/JUMP_DFU|interrupted:/);
  });

  it("says in plain words what state the meter may be in", async () => {
    const shown = text(view(await stopped()));
    expect(shown).toContain(STOPPED.message);
    expect(shown).toMatch(/bootloader/i);
    expect(shown).toMatch(/nothing was erased|not erased/i);
  });
});

// ---------------------------------------------------------------- AC-AXHK2M.14

describe("the next step after a stop (AC-AXHK2M.14)", () => {
  it("is a plain replug first, without the button", async () => {
    const steps = items(view(await stopped()));
    expect(steps.length).toBeGreaterThanOrEqual(2);
    expect(steps[0]).toMatch(/unplug|replug/i);
    expect(steps[0]).not.toMatch(/\bhold|button/i);
  });

  it("holds the button only if the replug does not bring the meter back", async () => {
    const steps = items(view(await stopped()));
    const hold = steps.findIndex((s) => /hold/i.test(s));
    expect(hold).toBeGreaterThan(0);
    expect(steps[hold]).toMatch(/\bif\b/i);
    expect(steps[hold]).toMatch(/does not|doesn't|not come back|still/i);
  });

  it("lists a meter in bootloader mode as such", () => {
    expect(deviceOptionLabel(listing(9, "bootloader"), false)).toMatch(/bootloader mode/i);
    expect(deviceOptionLabel(listing(9, "bootloader"), true)).toMatch(/bootloader mode/i);
  });

  it("keeps the updated meter picked while it is listed in bootloader mode", async () => {
    const fw = await stopped();
    expect(fw.followTarget([listing(9, "bootloader"), DEMO])).toBe("usb:1:9");
  });
});

// ---------------------------------------------------------------- AC-AXHK2M.15

describe("connecting again after an update (AC-AXHK2M.15)", () => {
  it("carries the picked meter's serial", () => {
    expect(flashMeter(listing(4), null)?.serial).toBe(SERIAL);
  });

  it("after a stop: waits while the meter is in its bootloader, then reconnects by serial", async () => {
    const b = fakeBackend();
    const fw = await stopped(b);

    expect(fw.followTarget([DEMO])).toBeNull();
    expect(fw.followTarget([listing(9, "bootloader"), DEMO])).toBe("usb:1:9");
    expect(b.reconnect).not.toHaveBeenCalled();

    // Back in application mode at a new address, with the same serial.
    expect(fw.followTarget([listing(10), OTHER, DEMO])).toBe("usb:1:10");
    expect(b.reconnect).toHaveBeenCalledOnce();
    expect(b.reconnect).toHaveBeenCalledWith("usb:1:10");
  });

  it("after a done update: reconnects to the same meter by its serial, once", async () => {
    const b = fakeBackend();
    const fw = await done(b);
    expect(fw.followTarget([listing(11), DEMO])).toBe("usb:1:11");
    expect(b.reconnect).toHaveBeenCalledExactlyOnceWith("usb:1:11");
    fw.followTarget([listing(11), DEMO]);
    fw.followTarget([listing(11), OTHER, DEMO]);
    expect(b.reconnect).toHaveBeenCalledOnce();
  });

  it("never connects another meter in its place", async () => {
    const b = fakeBackend();
    const fw = await done(b);
    expect(fw.followTarget([OTHER, DEMO])).toBeNull();
    expect(b.reconnect).not.toHaveBeenCalled();
  });

  it("does not reconnect while the update still runs", async () => {
    const b = fakeBackend();
    const fw = await running(b);
    fw.followTarget([listing(4), DEMO]);
    expect(b.reconnect).not.toHaveBeenCalled();
  });

  it("does nothing when no update ran", () => {
    const b = fakeBackend();
    const fw = new FirmwareUpdate(b.api);
    expect(fw.followTarget([listing(4), DEMO])).toBeNull();
    expect(b.reconnect).not.toHaveBeenCalled();
  });

  it("reconnects an updated demo device by its id", async () => {
    const b = fakeBackend();
    const demo = flashMeter(DEMO, details("2.0.6", "2.5"))!;
    const fw = await done(b, demo);
    expect(fw.followTarget([listing(4), DEMO])).toBe(DEMO.id);
    expect(b.reconnect).toHaveBeenCalledExactlyOnceWith(DEMO.id);
  });
});

// ---------------------------------------------------------------- AC-AXHK2M.16

describe("the versions after an update (AC-AXHK2M.16)", () => {
  const rows = (m: FirmwareMeter | null) =>
    Object.fromEntries((m ? firmwareMeterRows(m) : []).map((r) => [r.label, r.value]));

  it("shows the version read back, not a dash, before the reconnect", async () => {
    const fw = await done();
    const back = flashMeter(listing(10), null);
    const shown = rows(fw.meterShown(back));
    expect(shown.Firmware).toBe("2.0.7");
    expect(shown.Bootloader).not.toBe("—");
    expect(shown.Bootloader).toBeTruthy();
  });

  it("shows versions, not a dash, after a stop", async () => {
    const fw = await stopped();
    const shown = rows(fw.meterShown(flashMeter(listing(9, "bootloader"), null)));
    expect(shown.Firmware).not.toBe("—");
    expect(shown.Bootloader).not.toBe("—");
  });

  it("shows the versions the reconnect read, over any kept ones", async () => {
    const fw = await done();
    const shown = rows(fw.meterShown(flashMeter(listing(10), details("2.0.7", "2.6"))));
    expect(shown).toMatchObject({ Firmware: "2.0.7", Bootloader: "2.6" });
  });

  it("shows another picked meter's own state, not the updated one's", async () => {
    const fw = await done();
    const shown = rows(fw.meterShown(flashMeter(OTHER, null)));
    expect(shown).toMatchObject({ Firmware: "—", Bootloader: "—" });
  });
});
