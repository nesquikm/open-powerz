/** FR M078V5: the launch file's command over Tauri IPC, the mock and the web build
 * (AC-M078V5.1, .3, .5). */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { invoke, fakeWeb, createWebBackend } = vi.hoisted(() => {
  const fakeWeb = {
    features: null as unknown,
    openRecordingFile: vi.fn(async () => ({
      name: "picked.csv",
      rows: 3,
      skipped: 0,
      left_out: 0,
      stats: null,
    })),
  };
  return { invoke: vi.fn(), fakeWeb, createWebBackend: vi.fn(async () => fakeWeb) };
});

vi.mock("@tauri-apps/api/core", () => {
  class Channel {
    onmessage: (msg: unknown) => void = () => {};
  }
  return { invoke, Channel, isTauri: () => true };
});

vi.mock("./web/webBackend", () => ({ createWebBackend }));

beforeEach(() => {
  vi.resetModules();
  invoke.mockReset();
  invoke.mockResolvedValue(null);
  createWebBackend.mockClear();
  fakeWeb.openRecordingFile.mockClear();
});

afterEach(() => {
  vi.unstubAllEnvs();
});

describe("openLaunchRecordingFile on the desktop (AC-M078V5.1, .3)", () => {
  it("asks recording_file_open_launch, with progress, and opens no dialog command", async () => {
    const b = await import("./backend");
    const summary = { name: "run.csv", rows: 3, skipped: 0, left_out: 0, stats: null };
    invoke.mockResolvedValueOnce(summary);
    const seen: unknown[] = [];
    expect(await b.openLaunchRecordingFile((p) => seen.push(p))).toEqual(summary);
    expect(invoke).toHaveBeenCalledTimes(1);
    const [command, args] = invoke.mock.calls[0] as [
      string,
      { onProgress: { onmessage: (m: unknown) => void } },
    ];
    expect(command).toBe("recording_file_open_launch");
    args.onProgress.onmessage({ done: 1, total: 2 });
    expect(seen).toEqual([{ done: 1, total: 2 }]);
  });

  it("resolves null when no file was handed over", async () => {
    const b = await import("./backend");
    invoke.mockResolvedValueOnce(null);
    expect(await b.openLaunchRecordingFile(() => {})).toBeNull();
  });

  it("a refusal rejects, as Open recording's does", async () => {
    const b = await import("./backend");
    invoke.mockRejectedValueOnce({ kind: "usage", message: "Not a recorder header.", hint: null });
    await expect(b.openLaunchRecordingFile(() => {})).rejects.toMatchObject({
      message: "Not a recorder header.",
    });
  });
});

describe("openLaunchRecordingFile in the web build (AC-M078V5.5)", () => {
  it("resolves null and neither invokes Tauri nor opens the browser's picker", async () => {
    vi.stubEnv("VITE_BACKEND", "web");
    const b = await import("./backend");
    expect(await b.openLaunchRecordingFile(() => {})).toBeNull();
    expect(invoke).not.toHaveBeenCalled();
    expect(fakeWeb.openRecordingFile).not.toHaveBeenCalled();
  });
});

describe("openLaunchRecordingFile on the mock backend", () => {
  it("resolves null with no ?launchfile= and invokes nothing", async () => {
    vi.stubEnv("VITE_BACKEND", "mock");
    const b = await import("./backend");
    expect(await b.openLaunchRecordingFile(() => {})).toBeNull();
    expect(invoke).not.toHaveBeenCalled();
  });
});
