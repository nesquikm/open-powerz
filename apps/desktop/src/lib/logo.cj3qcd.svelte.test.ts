/** The Startup logo view's read across writes and while it runs (FR CJ3QCD): a write
 * that succeeded drops the earlier read until the logo is read again (AC-CJ3QCD.1), and
 * a read reports how many of its chunks are done of the total while it runs
 * (AC-CJ3QCD.2). Fake backends; nothing reaches a meter. */

import { afterEach, describe, expect, it, vi } from "vitest";
import type {
  LogoPreview,
  LogoProgress,
  LogoRead,
  LogoSettingsCheck,
  LogoWritten,
  MeterSettings,
} from "./backend";
import { StartupLogoState, type LogoMeter } from "./logo.svelte";

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

const SAVED: LogoWritten = {
  duration_before: "off",
  duration_set: "3 s",
  settings: "saved",
  startup_duration: "3 s",
};

const NEEDS_SAVE: LogoWritten = { ...SAVED, settings: "needs_menu_save" };

const OLD_LOGO: LogoRead = { status: "logo", width: 80, height: 40, mode: "opaque", image: null };
const NEW_LOGO: LogoRead = { status: "logo", width: 100, height: 60, mode: "opaque", image: null };

type ReadFn = (full: boolean, onProgress?: (p: LogoProgress) => void) => Promise<LogoRead>;

function harness(written: LogoWritten = SAVED) {
  const deps = {
    choose: vi.fn(async (): Promise<string | null> => "/pics/logo.png"),
    preview: vi.fn(async (): Promise<LogoPreview> => PREVIEW),
    read: vi.fn<ReadFn>(async () => OLD_LOGO),
    upload: vi.fn(async (_p: string, _a: boolean, cb: (p: LogoProgress) => void) => {
      cb({ step: "chunks", done: 1, total: 1 });
      return written;
    }),
    remove: vi.fn(async (cb: (p: LogoProgress) => void) => {
      cb({ step: "chunks", done: 1, total: 1 });
      return written;
    }),
    writeSettings: vi.fn(async (): Promise<MeterSettings> => SETTINGS),
    checkSettings: vi.fn(async (): Promise<LogoSettingsCheck> => ({
      settings: "needs_menu_save",
      startup_duration: null,
    })),
    refreshSettings: vi.fn(async (): Promise<void> => {}),
  };
  const meter: LogoMeter = { model: "KM003C", settings: SETTINGS };
  const state = new StartupLogoState(deps, () => meter);
  return { deps, state };
}

/** Lets the started write run to its end (it resolves on fakes). */
async function flush(): Promise<void> {
  for (let i = 0; i < 10; i++) await Promise.resolve();
}

async function upload(state: StartupLogoState): Promise<void> {
  await state.choose();
  state.askUpload();
  await state.confirm("upload");
  await flush();
}

async function remove(state: StartupLogoState): Promise<void> {
  state.askRemove();
  await state.confirm("remove");
  await flush();
}

afterEach(() => {
  vi.useRealTimers();
});

describe("a write that succeeded drops the earlier read (AC-CJ3QCD.1)", () => {
  it("cj3qcd: after an upload the earlier read is gone", async () => {
    const h = harness();
    await h.state.read();
    expect(h.state.area).toEqual(OLD_LOGO);
    await upload(h.state);
    expect(h.state.phase).toBe("done");
    expect(h.state.area).toBeNull();
  });

  it("cj3qcd: after a remove the earlier read is gone", async () => {
    const h = harness();
    await h.state.read();
    expect(h.state.area).toEqual(OLD_LOGO);
    await remove(h.state);
    expect(h.state.phase).toBe("done");
    expect(h.state.area).toBeNull();
  });

  it("cj3qcd: a write waiting for the menu save also drops the earlier read", async () => {
    vi.useFakeTimers();
    const h = harness(NEEDS_SAVE);
    await h.state.read();
    await upload(h.state);
    expect(h.state.phase).toBe("saving");
    expect(h.state.area).toBeNull();
    h.state.stopChecking();
  });

  it("cj3qcd: reading again after the write shows the new read", async () => {
    const h = harness();
    await h.state.read();
    await upload(h.state);
    expect(h.state.area).toBeNull();
    h.deps.read.mockImplementation(async () => NEW_LOGO);
    await h.state.read();
    expect(h.state.area).toEqual(NEW_LOGO);
  });
});

describe("a read reports its chunks while it runs (AC-CJ3QCD.2)", () => {
  it("cj3qcd: the read's progress shows done of total while the read runs, then clears", async () => {
    const h = harness();
    let report: ((p: LogoProgress) => void) | undefined;
    let finish: ((r: LogoRead) => void) | undefined;
    h.deps.read.mockImplementation(
      (_full, onProgress) =>
        new Promise<LogoRead>((resolve) => {
          report = onProgress;
          finish = resolve;
        }),
    );
    const reading = h.state.read(true);
    expect(h.deps.read).toHaveBeenCalledTimes(1);
    expect(h.deps.read.mock.calls[0]?.[0]).toBe(true);
    expect(typeof report).toBe("function");
    expect(h.state.reading).toBe(true);
    report?.({ step: "chunks", done: 3, total: 240 });
    expect(h.state.readProgress).toEqual({ step: "chunks", done: 3, total: 240 });
    report?.({ step: "chunks", done: 120, total: 240 });
    expect(h.state.readProgress).toEqual({ step: "chunks", done: 120, total: 240 });
    finish?.(NEW_LOGO);
    await reading;
    expect(h.state.area).toEqual(NEW_LOGO);
    expect(h.state.readProgress).toBeNull();
  });

  it("cj3qcd: a new read starts with no progress left from the last one", async () => {
    const h = harness();
    h.deps.read.mockImplementation(async (_full, onProgress) => {
      onProgress?.({ step: "chunks", done: 5, total: 5 });
      return NEW_LOGO;
    });
    await h.state.read(true);
    expect(h.state.readProgress).toBeNull();
    let seen: LogoProgress | null | undefined;
    h.deps.read.mockImplementation(async () => {
      seen = h.state.readProgress;
      return NEW_LOGO;
    });
    await h.state.read(true);
    expect(seen).toBeNull();
  });
});
