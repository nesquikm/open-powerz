/** The Startup logo view's state (FR MEHQC0, AC .1 to .7): preview, typed confirmations,
 * a write that cannot be stopped and locks every meter control, the outcome with the
 * failed step and chunk, the startup-duration hint with its 1/3/5 s fix, the disabled
 * reasons, and Read. All on fake backends; nothing reaches a meter. */

import { describe, expect, it, vi } from "vitest";
import type {
  LogoPreview,
  LogoProgress,
  LogoRead,
  LogoWritten,
  MeterSettings,
  SettingChanges,
  UiError,
} from "./backend";
import {
  REMOVE_WORD,
  StartupLogoState,
  UPLOAD_WORD,
  confirmWord,
  logoFailure,
  type LogoMeter,
} from "./logo.svelte";

function settings(startup: string): MeterSettings {
  return {
    fields: [
      { key: "language", label: "Language", value: "English", tag: "confirmed" },
      { key: "startup_logo", label: "Startup logo", value: startup, tag: "confirmed" },
    ],
  };
}

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

const WRITTEN: LogoWritten = { startup_duration: "off", startup_screen_off: true };

const CHUNK_FAIL: UiError = {
  kind: "protocol",
  message: "Writing the logo stopped at chunk 7: REJECT.",
  hint: "The logo flag was not set, so the meter keeps its old screen. Try again.",
};

/** A deferred promise the test settles. */
function deferred<T>() {
  let resolve!: (v: T) => void;
  let reject!: (e: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  // A rejection the state never awaited must not fail the run on its own.
  promise.catch(() => {});
  return { promise, resolve, reject };
}

function harness(meter: LogoMeter = { model: "KM003C", settings: settings("off") }) {
  let onProgress: ((p: LogoProgress) => void) | null = null;
  const write = deferred<LogoWritten>();
  const deps = {
    choose: vi.fn(async (): Promise<string | null> => "/pics/logo.png"),
    preview: vi.fn(async (_path: string, alpha: boolean): Promise<LogoPreview> =>
      alpha ? { ...PREVIEW, mode: "alpha", blob_size: 18_016, chunks: 71 } : PREVIEW,
    ),
    read: vi.fn(async (full: boolean): Promise<LogoRead> => ({
      status: "logo",
      width: 100,
      height: 60,
      mode: "opaque",
      image: full ? "AAAA" : null,
    })),
    upload: vi.fn((_p: string, _a: boolean, cb: (p: LogoProgress) => void) => {
      onProgress = cb;
      return write.promise;
    }),
    remove: vi.fn((cb: (p: LogoProgress) => void) => {
      onProgress = cb;
      return write.promise;
    }),
    writeSettings: vi.fn(async (changes: SettingChanges): Promise<MeterSettings> =>
      settings(
        ["off", "1 s", "3 s", "5 s"][(changes as Record<string, number>)["startup_duration"] ?? 0]!,
      ),
    ),
  };
  const current = { ...meter };
  const state = new StartupLogoState(deps, () => current);
  return { deps, state, write, current, progress: (p: LogoProgress) => onProgress?.(p) };
}

async function previewed() {
  const h = harness();
  await h.state.choose();
  return h;
}

describe("confirmation words (AC-MEHQC0.2)", () => {
  it("asks for upload and remove", () => {
    expect(UPLOAD_WORD).toBe("upload");
    expect(REMOVE_WORD).toBe("remove");
    expect(confirmWord("upload")).toBe(UPLOAD_WORD);
    expect(confirmWord("remove")).toBe(REMOVE_WORD);
  });
});

describe("preview (AC-MEHQC0.1)", () => {
  it("chooses an image and previews it opaque", async () => {
    const h = harness();
    await h.state.choose();
    expect(h.deps.choose).toHaveBeenCalledTimes(1);
    expect(h.deps.preview).toHaveBeenCalledWith("/pics/logo.png", false);
    expect(h.state.path).toBe("/pics/logo.png");
    expect(h.state.preview).toEqual(PREVIEW);
    expect(h.state.phase).toBe("previewed");
  });

  it("a cancelled dialog keeps the view as it was", async () => {
    const h = harness();
    h.deps.choose.mockResolvedValueOnce(null);
    await h.state.choose();
    expect(h.deps.choose).toHaveBeenCalledTimes(1);
    expect(h.deps.preview).not.toHaveBeenCalled();
    expect(h.state.phase).toBe("idle");
    expect(h.state.preview).toBeNull();
  });

  it("switching to transparent previews the same file again in alpha mode", async () => {
    const h = await previewed();
    await h.state.setAlpha(true);
    expect(h.state.alpha).toBe(true);
    expect(h.deps.preview).toHaveBeenLastCalledWith("/pics/logo.png", true);
    expect(h.state.preview?.mode).toBe("alpha");
    expect(h.state.preview?.chunks).toBe(71);
  });

  it("a refused file shows the error and no preview", async () => {
    const h = harness();
    h.deps.preview.mockRejectedValueOnce({ kind: "usage", message: "cannot load image file" });
    await h.state.choose();
    expect(h.state.preview).toBeNull();
    expect(h.state.error?.message).toMatch(/cannot load/);
    expect(h.state.uploadDisabledReason).not.toBeNull();
  });
});

describe("typed confirmation (AC-MEHQC0.2)", () => {
  it("Upload opens a confirmation that sends nothing until the word is typed", async () => {
    const h = await previewed();
    h.state.askUpload();
    expect(h.state.phase).toBe("confirming");
    expect(h.state.confirming).toBe("upload");
    await h.state.confirm("Upload");
    await h.state.confirm("uploa");
    await h.state.confirm("");
    expect(h.deps.upload).not.toHaveBeenCalled();
    await h.state.confirm(UPLOAD_WORD);
    expect(h.deps.upload).toHaveBeenCalledTimes(1);
    expect(h.deps.upload.mock.calls[0]![0]).toBe("/pics/logo.png");
    expect(h.deps.upload.mock.calls[0]![1]).toBe(false);
  });

  it("Remove opens its own confirmation, also without a preview", async () => {
    const h = harness();
    h.state.askRemove();
    expect(h.state.confirming).toBe("remove");
    await h.state.confirm(UPLOAD_WORD);
    expect(h.deps.remove).not.toHaveBeenCalled();
    void h.state.confirm(REMOVE_WORD);
    await Promise.resolve();
    expect(h.deps.remove).toHaveBeenCalledTimes(1);
    expect(h.deps.upload).not.toHaveBeenCalled();
  });

  it("cancel closes the confirmation and keeps the preview", async () => {
    const h = await previewed();
    h.state.askUpload();
    h.state.cancelConfirm();
    expect(h.state.confirming).toBeNull();
    expect(h.state.phase).toBe("previewed");
    expect(h.state.preview).toEqual(PREVIEW);
    expect(h.deps.upload).not.toHaveBeenCalled();
  });
});

describe("during a write (AC-MEHQC0.3)", () => {
  it("shows chunk then read-back progress and is busy until the answer", async () => {
    const h = await previewed();
    h.state.askUpload();
    void h.state.confirm(UPLOAD_WORD);
    await Promise.resolve();
    expect(h.state.phase).toBe("writing");
    expect(h.state.busy).toBe(true);
    expect(h.state.action).toBe("upload");
    h.progress({ step: "chunks", done: 12, total: 47 });
    expect(h.state.progress).toEqual({ step: "chunks", done: 12, total: 47 });
    h.progress({ step: "read_back", done: 3, total: 6 });
    expect(h.state.progress).toEqual({ step: "read_back", done: 3, total: 6 });
    h.write.resolve(WRITTEN);
    await vi.waitFor(() => expect(h.state.phase).toBe("done"));
    expect(h.state.busy).toBe(false);
  });

  it("offers no way to stop: every meter control is refused while it runs", async () => {
    const h = await previewed();
    h.state.askUpload();
    void h.state.confirm(UPLOAD_WORD);
    await Promise.resolve();
    expect(h.state).not.toHaveProperty("cancel");
    expect(h.state.uploadDisabledReason).not.toBeNull();
    expect(h.state.removeDisabledReason).not.toBeNull();
    await h.state.choose();
    await h.state.setAlpha(true);
    h.state.askRemove();
    h.state.askUpload();
    await h.state.read(true);
    await h.state.setDuration(3);
    expect(h.deps.choose).toHaveBeenCalledTimes(1);
    expect(h.deps.preview).toHaveBeenCalledTimes(1);
    expect(h.deps.read).not.toHaveBeenCalled();
    expect(h.deps.writeSettings).not.toHaveBeenCalled();
    expect(h.deps.remove).not.toHaveBeenCalled();
    expect(h.deps.upload).toHaveBeenCalledTimes(1);
    expect(h.state.phase).toBe("writing");
    h.write.resolve(WRITTEN);
    await vi.waitFor(() => expect(h.state.phase).toBe("done"));
  });
});

describe("after a write (AC-MEHQC0.4)", () => {
  it("a verified upload ends done with what was written", async () => {
    const h = await previewed();
    h.state.askUpload();
    void h.state.confirm(UPLOAD_WORD);
    h.write.resolve(WRITTEN);
    await vi.waitFor(() => expect(h.state.phase).toBe("done"));
    expect(h.state.written).toEqual(WRITTEN);
    expect(h.state.failure).toBeNull();
  });

  it("a failure at chunk k names the step and the chunk and advises Upload or Remove", async () => {
    const h = await previewed();
    h.state.askUpload();
    void h.state.confirm(UPLOAD_WORD);
    await Promise.resolve();
    h.progress({ step: "chunks", done: 7, total: 47 });
    h.write.reject(CHUNK_FAIL);
    await vi.waitFor(() => expect(h.state.phase).toBe("failed"));
    expect(h.state.failure?.chunk).toBe(7);
    expect(h.state.failure?.step).toMatch(/chunk/i);
    expect(h.state.failure?.advice).toMatch(/upload/i);
    expect(h.state.failure?.advice).toMatch(/remove/i);
    expect(h.state.error?.message).toBe(CHUNK_FAIL.message);
    // Upload and Remove are offered again.
    expect(h.state.uploadDisabledReason).toBeNull();
    expect(h.state.removeDisabledReason).toBeNull();
  });

  it("logoFailure reads each failed step from the backend's error", () => {
    const chunk = logoFailure(CHUNK_FAIL, "upload");
    expect(chunk.chunk).toBe(7);
    expect(chunk.step).toMatch(/chunk/i);
    const flag = logoFailure(
      {
        kind: "protocol",
        message: "The logo was written but its flag write failed: REJECT.",
        hint: null,
      },
      "upload",
    );
    expect(flag.step).toMatch(/flag/i);
    expect(flag.chunk).toBeNull();
    const mismatch = logoFailure(
      {
        kind: "protocol",
        message: "The logo read back from the meter differs from the one written, at offset 0x120.",
        hint: null,
      },
      "upload",
    );
    expect(mismatch.step).toMatch(/read.?back/i);
    const remove = logoFailure(
      {
        kind: "protocol",
        message: "Removing the logo failed at the clear step: REJECT.",
        hint: null,
      },
      "remove",
    );
    expect(remove.step).toMatch(/clear/i);
    for (const f of [chunk, flag, mismatch, remove]) {
      expect(f.advice).toMatch(/upload/i);
      expect(f.advice).toMatch(/remove/i);
    }
  });
});

describe("startup-screen duration (AC-MEHQC0.5)", () => {
  it("shows the duration from the settings read", () => {
    const h = harness({ model: "KM003C", settings: settings("3 s") });
    expect(h.state.startupDuration).toBe("3 s");
    expect(h.state.durationOff).toBe(false);
  });

  it("says off when the startup screen is off", () => {
    const h = harness();
    expect(h.state.startupDuration).toBe("off");
    expect(h.state.durationOff).toBe(true);
  });

  it.each([
    [1, 1, "1 s"],
    [3, 2, "3 s"],
    [5, 3, "5 s"],
  ] as const)(
    "%i s goes out as startup_duration %i through the settings write",
    async (s, code, shown) => {
      const h = harness();
      await h.state.setDuration(s);
      expect(h.deps.writeSettings).toHaveBeenCalledTimes(1);
      expect(h.deps.writeSettings).toHaveBeenCalledWith({ startup_duration: code });
      expect(h.state.startupDuration).toBe(shown);
      expect(h.state.durationOff).toBe(false);
    },
  );

  it("a failed duration write shows the error and keeps it off", async () => {
    const h = harness();
    h.deps.writeSettings.mockRejectedValueOnce({ kind: "protocol", message: "mismatch" });
    await h.state.setDuration(3);
    expect(h.state.error?.message).toBe("mismatch");
    expect(h.state.durationOff).toBe(true);
  });
});

describe("disabled reasons (AC-MEHQC0.6)", () => {
  it("a KM003C with settings read and a preview may upload and remove", async () => {
    const h = await previewed();
    expect(h.state.preview).toEqual(PREVIEW);
    expect(h.state.uploadDisabledReason).toBeNull();
    expect(h.state.removeDisabledReason).toBeNull();
  });

  it("Upload needs an image first; Remove does not", () => {
    const h = harness();
    expect(h.state.uploadDisabledReason).toMatch(/image|picture/i);
    expect(h.state.removeDisabledReason).toBeNull();
  });

  it("another model is refused naming the KM003C", async () => {
    const h = harness({ model: "KM002C", settings: settings("off") });
    await h.state.choose();
    expect(h.state.uploadDisabledReason).toMatch(/KM003C/);
    expect(h.state.removeDisabledReason).toMatch(/KM003C/);
    h.state.askUpload();
    h.state.askRemove();
    expect(h.state.confirming).toBeNull();
  });

  it("before the settings were read both are disabled with that reason", async () => {
    const h = harness({ model: "KM003C", settings: null });
    await h.state.choose();
    expect(h.state.uploadDisabledReason).toMatch(/settings/i);
    expect(h.state.removeDisabledReason).toMatch(/settings/i);
    h.current.settings = settings("off");
    expect(h.state.uploadDisabledReason).toBeNull();
  });

  it("before the model was read both are disabled", async () => {
    const h = harness({ model: null, settings: settings("off") });
    await h.state.choose();
    expect(h.state.uploadDisabledReason).toMatch(/model|read/i);
    expect(h.state.removeDisabledReason).toMatch(/model|read/i);
  });
});

describe("Read (AC-MEHQC0.7)", () => {
  it("shows what the logo area holds", async () => {
    const h = harness();
    h.deps.read.mockResolvedValueOnce({
      status: "empty",
      width: null,
      height: null,
      mode: null,
      image: null,
    });
    await h.state.read();
    expect(h.deps.read).toHaveBeenCalledWith(false);
    expect(h.state.area?.status).toBe("empty");
  });

  it("can show the stored logo", async () => {
    const h = harness();
    await h.state.read(true);
    expect(h.deps.read).toHaveBeenCalledWith(true);
    expect(h.state.area).toMatchObject({ status: "logo", width: 100, height: 60, image: "AAAA" });
  });

  it("an unrecognised area is shown as such", async () => {
    const h = harness();
    h.deps.read.mockResolvedValueOnce({
      status: "unrecognised",
      width: null,
      height: null,
      mode: null,
      image: null,
    });
    await h.state.read();
    expect(h.state.area?.status).toBe("unrecognised");
  });
});
