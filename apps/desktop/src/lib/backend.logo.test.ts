/** The startup logo over Tauri IPC (FR MEHQC0, AC .1, .3, .7 and .8): the preview needs
 * no meter; read, upload and Remove name the meter, and the writes stream their batched
 * progress over a channel. */

import { beforeEach, describe, expect, it, vi } from "vitest";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => {
  class Channel {
    onmessage: (msg: unknown) => void = () => {};
  }
  return { invoke, Channel, isTauri: () => true };
});

import {
  logoChoose,
  logoPreview,
  logoRead,
  logoRemove,
  logoSettingsCheck,
  logoUpload,
  type LogoPreview,
  type LogoProgress,
  type LogoWritten,
} from "./backend";

beforeEach(() => {
  invoke.mockReset();
});

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
  settings: "needs_menu_save",
  startup_duration: null,
};

describe("logo preview (AC-MEHQC0.1)", () => {
  it("chooses a file with the backend's open dialog", async () => {
    invoke.mockResolvedValue("/pics/logo.png");
    await expect(logoChoose()).resolves.toBe("/pics/logo.png");
    expect(invoke).toHaveBeenCalledWith("logo_choose");
  });

  it("previews a path in the chosen mode", async () => {
    invoke.mockResolvedValue(PREVIEW);
    await expect(logoPreview("/pics/logo.png", true)).resolves.toEqual(PREVIEW);
    expect(invoke).toHaveBeenCalledWith("logo_preview", { path: "/pics/logo.png", alpha: true });
  });
});

describe("logo read (AC-MEHQC0.7)", () => {
  it("reads the meter's logo area, with the image when asked", async () => {
    invoke.mockResolvedValue({
      status: "empty",
      width: null,
      height: null,
      mode: null,
      image: null,
    });
    await logoRead("demo:load-5v-1a", true);
    expect(invoke).toHaveBeenCalledWith("logo_read", { deviceId: "demo:load-5v-1a", full: true });
  });
});

describe("logo writes (AC-MEHQC0.3, .8)", () => {
  it("upload sends the path, mode and a progress channel and resolves to what was written", async () => {
    const seen: LogoProgress[] = [];
    invoke.mockImplementation(async (_cmd: string, args: Record<string, unknown>) => {
      const channel = args["onProgress"] as { onmessage: (m: LogoProgress) => void };
      channel.onmessage({ step: "chunks", done: 1, total: 47 });
      channel.onmessage({ step: "read_back", done: 6, total: 6 });
      return WRITTEN;
    });
    await expect(
      logoUpload("demo:load-5v-1a", "/pics/logo.png", false, (p) => seen.push(p)),
    ).resolves.toEqual(WRITTEN);
    expect(invoke).toHaveBeenCalledTimes(1);
    const [cmd, args] = invoke.mock.calls[0]!;
    expect(cmd).toBe("logo_upload");
    expect(args).toMatchObject({
      deviceId: "demo:load-5v-1a",
      path: "/pics/logo.png",
      alpha: false,
    });
    expect(seen).toEqual([
      { step: "chunks", done: 1, total: 47 },
      { step: "read_back", done: 6, total: 6 },
    ]);
  });

  it("remove sends a progress channel too", async () => {
    const seen: LogoProgress[] = [];
    invoke.mockImplementation(async (_cmd: string, args: Record<string, unknown>) => {
      (args["onProgress"] as { onmessage: (m: LogoProgress) => void }).onmessage({
        step: "chunks",
        done: 1,
        total: 1,
      });
      return WRITTEN;
    });
    await logoRemove("demo:load-5v-1a", (p) => seen.push(p));
    const [cmd, args] = invoke.mock.calls[0]!;
    expect(cmd).toBe("logo_remove");
    expect(args).toMatchObject({ deviceId: "demo:load-5v-1a" });
    expect(seen).toEqual([{ step: "chunks", done: 1, total: 1 }]);
  });

  it("passes a failed write on as the backend error", async () => {
    const err = {
      kind: "protocol",
      message: "Writing the logo stopped at chunk 7: REJECT.",
      hint: null,
    };
    invoke.mockRejectedValue(err);
    await expect(logoUpload("demo:load-5v-1a", "/x.png", false, () => {})).rejects.toEqual(err);
  });
});

describe("logo settings check (FR QG6RNY, AC .2, .3)", () => {
  it("qg6rny: asks the backend for one check of meter deviceId's settings", async () => {
    invoke.mockResolvedValue({ settings: "needs_menu_save", startup_duration: null });
    await expect(logoSettingsCheck("demo:load-5v-1a")).resolves.toEqual({
      settings: "needs_menu_save",
      startup_duration: null,
    });
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(invoke).toHaveBeenCalledWith("logo_settings_check", { deviceId: "demo:load-5v-1a" });
  });

  it("qg6rny: passes the saved duration and a refusal through", async () => {
    invoke.mockResolvedValueOnce({ settings: "saved", startup_duration: "3 s" });
    await expect(logoSettingsCheck("demo:load-5v-1a")).resolves.toEqual({
      settings: "saved",
      startup_duration: "3 s",
    });
    const err = {
      kind: "usage",
      message: "No logo write is waiting for a settings save.",
      hint: null,
    };
    invoke.mockRejectedValueOnce(err);
    await expect(logoSettingsCheck("demo:load-5v-1a")).rejects.toEqual(err);
  });
});

describe("the write result (FR QG6RNY, AC .1)", () => {
  it("qg6rny: upload resolves to the durations before and set and the settings state", async () => {
    invoke.mockResolvedValue(WRITTEN);
    const w = await logoUpload("demo:load-5v-1a", "/pics/logo.png", false, () => {});
    expect(w).toEqual({
      duration_before: "off",
      duration_set: "3 s",
      settings: "needs_menu_save",
      startup_duration: null,
    });
  });
});
