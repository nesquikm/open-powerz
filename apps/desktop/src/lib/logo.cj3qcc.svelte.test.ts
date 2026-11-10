/** The Startup logo view's state over HID and across connections (FR CJ3QCC): over HID
 * the Upload and Remove reasons are the HID note, not the model line; a KM003C named
 * with a product prefix (the USB product name, e.g. "POWER-Z KM003C") counts as a
 * KM003C while a KM002C still gets the model line; a new connection clears the tab's
 * previous error and logo read. Fake backends; nothing reaches a meter. */

import { describe, expect, it, vi } from "vitest";
import type {
  LogoPreview,
  LogoProgress,
  LogoRead,
  LogoSettingsCheck,
  LogoWritten,
  MeterSettings,
  UiError,
} from "./backend";
import { StartupLogoState, type LogoMeter } from "./logo.svelte";
import { hidOnlyBasicText } from "./web/features";

const HID_NOTE = hidOnlyBasicText("The startup logo");

const SETTINGS: MeterSettings = {
  fields: [{ key: "startup_logo", label: "Startup logo", value: "off", tag: "confirmed" }],
};

const PREVIEW: LogoPreview = {
  width: 100,
  height: 60,
  mode: "opaque",
  blob_size: 12_016,
  padded_size: 12_032,
  chunks: 47,
  small_side_warning: false,
  rgba: "AAAA",
};

const WRITTEN: LogoWritten = {
  duration_before: "off",
  duration_set: "3 s",
  settings: "saved",
  startup_duration: "3 s",
};

const STORED: LogoRead = { status: "empty", width: null, height: null, mode: null, image: null };

const REFUSED: UiError = { kind: "usage", message: "The picture could not be read.", hint: null };

function harness(meter: LogoMeter) {
  const deps = {
    choose: vi.fn(async (): Promise<string | null> => "/pics/logo.png"),
    preview: vi.fn(async (): Promise<LogoPreview> => PREVIEW),
    read: vi.fn(async (): Promise<LogoRead> => STORED),
    upload: vi.fn(async (_p: string, _a: boolean, cb: (p: LogoProgress) => void) => {
      cb({ step: "chunks", done: 1, total: 1 });
      return WRITTEN;
    }),
    remove: vi.fn(async (cb: (p: LogoProgress) => void) => {
      cb({ step: "chunks", done: 1, total: 1 });
      return WRITTEN;
    }),
    writeSettings: vi.fn(async (): Promise<MeterSettings> => SETTINGS),
    checkSettings: vi.fn(async (): Promise<LogoSettingsCheck> => ({
      settings: "saved",
      startup_duration: "3 s",
    })),
    refreshSettings: vi.fn(async (): Promise<void> => {}),
  };
  const current: LogoMeter = { ...meter };
  const state = new StartupLogoState(deps, () => current);
  return { deps, state, current };
}

describe("over HID the reasons are the HID note (AC-CJ3QCC.1)", () => {
  it("cj3qcc: a HID KM003C named by its USB product gets the HID note for Upload and Remove", async () => {
    const h = harness({ model: "POWER-Z KM003C", settings: SETTINGS, basicReadingsOnly: true });
    await h.state.choose();
    expect(h.state.uploadDisabledReason).toBe(HID_NOTE);
    expect(h.state.removeDisabledReason).toBe(HID_NOTE);
    expect(h.state.uploadDisabledReason).not.toContain("needs a KM003C");
  });

  it("cj3qcc: the HID note comes before the unread model and settings", () => {
    const h = harness({ model: null, settings: null, basicReadingsOnly: true });
    expect(h.state.uploadDisabledReason).toBe(HID_NOTE);
    expect(h.state.removeDisabledReason).toBe(HID_NOTE);
  });

  it("cj3qcc: over HID Remove sends nothing", () => {
    const h = harness({ model: "POWER-Z KM003C", settings: SETTINGS, basicReadingsOnly: true });
    h.state.askRemove();
    expect(h.state.confirming).toBeNull();
    expect(h.state.phase).toBe("idle");
  });

  it("cj3qcc: the same meter over WebUSB (not basic readings) has no reason", async () => {
    const h = harness({ model: "POWER-Z KM003C", settings: SETTINGS, basicReadingsOnly: false });
    await h.state.choose();
    expect(h.state.uploadDisabledReason).toBeNull();
    expect(h.state.removeDisabledReason).toBeNull();
  });
});

describe("the model check matches the model token (AC-CJ3QCC.2)", () => {
  it("cj3qcc: a KM003C with a product prefix counts as a KM003C", async () => {
    const h = harness({ model: "POWER-Z KM003C", settings: SETTINGS });
    expect(h.state.removeDisabledReason).toBeNull();
    await h.state.choose();
    expect(h.state.uploadDisabledReason).toBeNull();
    h.state.askRemove();
    expect(h.state.confirming).toBe("remove");
  });

  it("cj3qcc: a plain KM003C still counts as a KM003C", async () => {
    const h = harness({ model: "KM003C", settings: SETTINGS });
    await h.state.choose();
    expect(h.state.uploadDisabledReason).toBeNull();
    expect(h.state.removeDisabledReason).toBeNull();
  });

  it("cj3qcc: a KM002C, prefixed or not, still gets the model line", async () => {
    const prefixed = harness({ model: "POWER-Z KM002C", settings: SETTINGS });
    await prefixed.state.choose();
    expect(prefixed.state.uploadDisabledReason).toBe(
      "The startup logo needs a KM003C; this meter is a POWER-Z KM002C.",
    );
    expect(prefixed.state.removeDisabledReason).toBe(
      "The startup logo needs a KM003C; this meter is a POWER-Z KM002C.",
    );
    const plain = harness({ model: "KM002C", settings: SETTINGS });
    expect(plain.state.removeDisabledReason).toBe(
      "The startup logo needs a KM003C; this meter is a KM002C.",
    );
  });

  it("cj3qcc: a name that only contains KM003C inside a longer token is not a KM003C", () => {
    const h = harness({ model: "KM003CX", settings: SETTINGS });
    expect(h.state.removeDisabledReason).toBe(
      "The startup logo needs a KM003C; this meter is a KM003CX.",
    );
  });
});

describe("a new connection clears the tab (AC-CJ3QCC.3)", () => {
  it("cj3qcc: the previous error and logo read are gone after a new connection", async () => {
    // Read over WebUSB (FR Z99166: over HID Read sends nothing), then reconnect over HID.
    const h = harness({ model: "POWER-Z KM003C", settings: SETTINGS, basicReadingsOnly: false });
    await h.state.read();
    expect(h.state.area).toEqual(STORED);
    h.current.basicReadingsOnly = true;
    h.deps.preview.mockRejectedValueOnce(REFUSED);
    await h.state.choose();
    expect(h.state.error).toEqual(REFUSED);

    // Reconnect over WebUSB.
    h.current.basicReadingsOnly = false;
    h.state.resetSession();

    expect(h.state.error).toBeNull();
    expect(h.state.area).toBeNull();
    expect(h.state.removeDisabledReason).toBeNull();
  });

  it("cj3qcc: a failed read's error is cleared by a new connection", async () => {
    const h = harness({ model: "KM003C", settings: SETTINGS });
    h.deps.read.mockRejectedValueOnce(REFUSED);
    await h.state.read();
    expect(h.state.error).toEqual(REFUSED);
    h.state.resetSession();
    expect(h.state.error).toBeNull();
    expect(h.state.area).toBeNull();
  });
});
