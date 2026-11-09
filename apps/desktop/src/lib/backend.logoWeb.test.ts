/**
 * FR W8SHXB: in the web build (`VITE_BACKEND=web`) every startup logo call goes to the
 * web backend, with the desktop call's arguments, instead of the desktop-only refusal
 * (AC-W8SHXB.1, .5, .6, .7); the Tauri path keeps invoking its commands.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { invoke, createWebBackend, fakeWeb } = vi.hoisted(() => {
  const answer = (value: unknown) =>
    vi.fn(async (...args: unknown[]) => (args.length >= 0 ? value : null));
  const fakeWeb = {
    features: null as unknown,
    listDevices: vi.fn(async () => []),
    logoChoose: answer("web-file:1:logo.png"),
    logoPreview: answer({ width: 100 }),
    logoPreviewBuiltin: answer({ width: 200 }),
    logoRead: answer({ status: "empty" }),
    logoUpload: answer({ duration_set: "3 s" }),
    logoUploadBuiltin: answer({ duration_set: "3 s" }),
    logoRemove: answer({ duration_set: "off" }),
    logoSettingsCheck: answer({ settings: "saved", startup_duration: "3 s" }),
  };
  return {
    invoke: vi.fn(async () => null),
    createWebBackend: vi.fn(async () => fakeWeb),
    fakeWeb,
  };
});

vi.mock("@tauri-apps/api/core", () => {
  class Channel {
    onmessage: (msg: unknown) => void = () => {};
  }
  return { invoke, Channel, isTauri: () => true };
});
vi.mock("./web/webBackend", () => ({ createWebBackend }));
vi.mock("@tauri-apps/plugin-opener", () => ({ revealItemInDir: vi.fn(async () => {}) }));

beforeEach(() => {
  vi.resetModules();
  invoke.mockClear();
  createWebBackend.mockClear();
  for (const fn of Object.values(fakeWeb))
    if (typeof fn === "function") (fn as { mockClear?: () => void }).mockClear?.();
});

afterEach(() => {
  vi.unstubAllEnvs();
});

describe("web routing of the logo calls (AC-W8SHXB.1, .5, .6)", () => {
  it("logoChoose, logoPreview and logoPreviewBuiltin answer from the web backend", async () => {
    vi.stubEnv("VITE_BACKEND", "web");
    const b = await import("./backend");
    expect(await b.logoChoose()).toBe("web-file:1:logo.png");
    expect(fakeWeb.logoChoose).toHaveBeenCalledTimes(1);
    expect(await b.logoPreview("web-file:1:logo.png", true)).toEqual({ width: 100 });
    expect(fakeWeb.logoPreview).toHaveBeenCalledWith("web-file:1:logo.png", true);
    expect(await b.logoPreviewBuiltin("surf")).toEqual({ width: 200 });
    expect(fakeWeb.logoPreviewBuiltin).toHaveBeenCalledWith("surf");
    expect(invoke).not.toHaveBeenCalled();
  });

  it("logoRead and logoSettingsCheck answer from the web backend for the meter", async () => {
    vi.stubEnv("VITE_BACKEND", "web");
    const b = await import("./backend");
    expect(await b.logoRead("usb:1", true)).toEqual({ status: "empty" });
    expect(fakeWeb.logoRead).toHaveBeenCalledWith("usb:1", true);
    expect(await b.logoSettingsCheck("usb:1")).toEqual({
      settings: "saved",
      startup_duration: "3 s",
    });
    expect(fakeWeb.logoSettingsCheck).toHaveBeenCalledWith("usb:1");
    expect(invoke).not.toHaveBeenCalled();
  });

  it("the writes keep going to the web backend with their progress callback", async () => {
    vi.stubEnv("VITE_BACKEND", "web");
    const b = await import("./backend");
    const onProgress = () => {};
    await b.logoUpload("usb:1", "web-file:1:logo.png", false, onProgress);
    expect(fakeWeb.logoUpload).toHaveBeenCalledWith(
      "usb:1",
      "web-file:1:logo.png",
      false,
      onProgress,
    );
    await b.logoUploadBuiltin("usb:1", "surf", onProgress);
    expect(fakeWeb.logoUploadBuiltin).toHaveBeenCalledWith("usb:1", "surf", onProgress);
    await b.logoRemove("usb:1", onProgress);
    expect(fakeWeb.logoRemove).toHaveBeenCalledWith("usb:1", onProgress);
    expect(invoke).not.toHaveBeenCalled();
  });

  it("the web build's features switch the logo on", async () => {
    vi.stubEnv("VITE_BACKEND", "web");
    const b = await import("./backend");
    expect(b.features.logoUpload).toBe(true);
  });

  it("the Tauri path still invokes the desktop commands", async () => {
    const b = await import("./backend");
    await b.logoChoose();
    expect(invoke).toHaveBeenCalledWith("logo_choose");
    await b.logoRead("usb:1", false);
    expect(invoke).toHaveBeenCalledWith("logo_read", { deviceId: "usb:1", full: false });
    expect(fakeWeb.logoChoose).not.toHaveBeenCalled();
    expect(createWebBackend).not.toHaveBeenCalled();
  });
});
