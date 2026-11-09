/** The Startup logo view's state (FR MEHQC0, AC .1 to .7): preview, typed confirmations,
 * a write that cannot be stopped and locks every meter control, the outcome with the
 * failed step and chunk, the startup-duration hint with its 1/3/5 s fix, the disabled
 * reasons, and Read. All on fake backends; nothing reaches a meter. */

import { describe, expect, it, vi } from "vitest";
import type {
  BuiltinLogoName,
  LogoPreview,
  LogoProgress,
  LogoRead,
  LogoSettingsCheck,
  LogoWritten,
  MeterSettings,
  SettingChanges,
  UiError,
} from "./backend";
import {
  BUILTIN_LOGOS,
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

/** A built-in logo's preview: 200 × 200, transparent (FR 70X5YX). */
const BUILTIN_PREVIEW: LogoPreview = {
  width: 200,
  height: 200,
  mode: "alpha",
  blob_size: 120_016,
  padded_size: 120_064,
  chunks: 469,
  small_side_warning: false,
  rgba: "AAAA",
};

const WRITTEN: LogoWritten = {
  duration_before: "off",
  duration_set: "3 s",
  settings: "saved",
  startup_duration: "3 s",
};

const CHUNK_FAIL: UiError = {
  kind: "protocol",
  message: "Writing the logo stopped at chunk 7: REJECT.",
  hint: "The startup duration was not changed, so the meter keeps its old screen. Try again.",
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
    previewBuiltin: vi.fn(async (name: BuiltinLogoName): Promise<LogoPreview> =>
      name === "surf" ? BUILTIN_PREVIEW : { ...BUILTIN_PREVIEW, rgba: "BBBB" },
    ),
    uploadBuiltin: vi.fn((_name: BuiltinLogoName, cb: (p: LogoProgress) => void) => {
      onProgress = cb;
      return write.promise;
    }),
    remove: vi.fn((cb: (p: LogoProgress) => void) => {
      onProgress = cb;
      return write.promise;
    }),
    checkSettings: vi.fn(async (): Promise<LogoSettingsCheck> => ({
      settings: "saved",
      startup_duration: "3 s",
    })),
    refreshSettings: vi.fn(async (): Promise<void> => {}),
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
        message: "The logo was written but its startup duration write (0x1C) failed: REJECT.",
        hint: null,
      },
      "upload",
    );
    expect(flag.step).toMatch(/startup duration/i);
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
    // The full read also hears its chunk progress (AC-CJ3QCD.2).
    expect(h.deps.read).toHaveBeenCalledWith(true, expect.any(Function));
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

describe("built-in open-powerz logos (FR 70X5YX)", () => {
  it("AC-70X5YX.5: offers the mascot and the mascot with lettering", () => {
    expect(BUILTIN_LOGOS.map((b) => b.name)).toEqual(["surf", "surf-wordmark"]);
    const [surf, wordmark] = BUILTIN_LOGOS;
    expect(surf!.label).toMatch(/mascot/i);
    expect(wordmark!.label).toMatch(/mascot/i);
    expect(wordmark!.label).toMatch(/letter/i);
    expect(surf!.label).not.toBe(wordmark!.label);
  });

  it("AC-70X5YX.5: picking a built-in previews it as a chosen file does", async () => {
    const h = harness();
    await h.state.useBuiltin("surf");
    expect(h.deps.previewBuiltin).toHaveBeenCalledWith("surf");
    expect(h.deps.preview).not.toHaveBeenCalled();
    expect(h.deps.choose).not.toHaveBeenCalled();
    expect(h.state.builtin).toBe("surf");
    expect(h.state.path).toBeNull();
    expect(h.state.preview).toEqual(BUILTIN_PREVIEW);
    expect(h.state.phase).toBe("previewed");
    expect(h.state.uploadDisabledReason).toBeNull();
  });

  it("AC-70X5YX.5: an upload of a built-in never turns into a Remove, even with no uploadBuiltin wired", async () => {
    const h = harness();
    await h.state.useBuiltin("surf");
    delete (h.deps as Partial<typeof h.deps>).uploadBuiltin;
    h.state.askUpload();
    await h.state.confirm("upload");
    await Promise.resolve();
    expect(h.deps.remove).not.toHaveBeenCalled();
    expect(h.deps.upload).not.toHaveBeenCalled();
    expect(h.state.phase).toBe("failed");
  });

  it("AC-70X5YX.5: switching between the two built-ins previews the other one", async () => {
    const h = harness();
    await h.state.useBuiltin("surf");
    await h.state.useBuiltin("surf-wordmark");
    expect(h.deps.previewBuiltin).toHaveBeenLastCalledWith("surf-wordmark");
    expect(h.state.builtin).toBe("surf-wordmark");
    expect(h.state.preview?.rgba).toBe("BBBB");
  });

  it("AC-70X5YX.5: a refused built-in preview shows the error and no preview", async () => {
    const h = harness();
    h.deps.previewBuiltin.mockRejectedValueOnce({
      kind: "usage",
      message: "unknown built-in logo",
      hint: null,
    });
    await h.state.useBuiltin("surf");
    expect(h.state.preview).toBeNull();
    expect(h.state.error?.message).toMatch(/built-in/);
    expect(h.state.uploadDisabledReason).not.toBeNull();
  });

  it("AC-70X5YX.5: Upload of a built-in goes through the typed confirmation", async () => {
    const h = harness();
    await h.state.useBuiltin("surf-wordmark");
    h.state.askUpload();
    expect(h.state.confirming).toBe("upload");
    expect(h.state.uploadConfirmText).toBe("The startup duration goes from off to 3 s.");
    await h.state.confirm("Upload");
    expect(h.deps.uploadBuiltin).not.toHaveBeenCalled();
    await h.state.confirm(UPLOAD_WORD);
    expect(h.deps.uploadBuiltin).toHaveBeenCalledTimes(1);
    expect(h.deps.uploadBuiltin.mock.calls[0]![0]).toBe("surf-wordmark");
    expect(h.deps.upload).not.toHaveBeenCalled();
  });

  it("AC-70X5YX.5: a built-in upload shows progress, ends done and runs the settings check", async () => {
    vi.useFakeTimers();
    try {
      const h = harness();
      await h.state.useBuiltin("surf");
      h.state.askUpload();
      void h.state.confirm(UPLOAD_WORD);
      await Promise.resolve();
      expect(h.state.phase).toBe("writing");
      expect(h.state.busy).toBe(true);
      h.progress({ step: "chunks", done: 100, total: 469 });
      expect(h.state.progress).toEqual({ step: "chunks", done: 100, total: 469 });
      h.progress({ step: "read_back", done: 469, total: 469 });
      expect(h.state.progress).toEqual({ step: "read_back", done: 469, total: 469 });
      h.write.resolve({ ...WRITTEN, settings: "needs_menu_save", startup_duration: null });
      await vi.waitFor(() => expect(h.state.phase).toBe("saving"));
      await vi.advanceTimersByTimeAsync(2000);
      expect(h.deps.checkSettings).toHaveBeenCalledTimes(1);
      await vi.waitFor(() => expect(h.state.phase).toBe("done"));
      expect(h.state.written?.startup_duration).toBe("3 s");
    } finally {
      vi.useRealTimers();
    }
  });

  it("AC-70X5YX.5: a failed built-in upload names the chunk like a file upload", async () => {
    const h = harness();
    await h.state.useBuiltin("surf");
    h.state.askUpload();
    void h.state.confirm(UPLOAD_WORD);
    h.write.reject(CHUNK_FAIL);
    await vi.waitFor(() => expect(h.state.phase).toBe("failed"));
    expect(h.state.failure).toMatchObject({ step: "Writing a chunk", chunk: 7 });
  });

  it("AC-70X5YX.5: a built-in cannot be picked while a write runs", async () => {
    const h = await previewed();
    h.state.askUpload();
    void h.state.confirm(UPLOAD_WORD);
    await Promise.resolve();
    await h.state.useBuiltin("surf");
    expect(h.deps.previewBuiltin).not.toHaveBeenCalled();
    expect(h.state.builtin).toBeNull();
    h.write.resolve(WRITTEN);
    await vi.waitFor(() => expect(h.state.phase).toBe("done"));
  });

  it("AC-70X5YX.6: a built-in uses the transparent mode and disables opaque with a reason", async () => {
    const h = harness();
    expect(h.state.opaqueDisabledReason).toBeNull();
    await h.state.useBuiltin("surf");
    expect(h.state.alpha).toBe(true);
    expect(h.state.opaqueDisabledReason).toMatch(/transparent/i);
    await h.state.setAlpha(false);
    expect(h.state.alpha).toBe(true);
    expect(h.deps.preview).not.toHaveBeenCalled();
    expect(h.state.preview).toEqual(BUILTIN_PREVIEW);
  });

  it("AC-70X5YX.6: choosing a file after a built-in brings both modes back", async () => {
    const h = harness();
    await h.state.useBuiltin("surf");
    await h.state.choose();
    expect(h.state.builtin).toBeNull();
    expect(h.state.path).toBe("/pics/logo.png");
    expect(h.state.opaqueDisabledReason).toBeNull();
    await h.state.setAlpha(false);
    expect(h.state.alpha).toBe(false);
    expect(h.deps.preview).toHaveBeenLastCalledWith("/pics/logo.png", false);
    expect(h.state.preview).toEqual(PREVIEW);
    await h.state.setAlpha(true);
    expect(h.deps.preview).toHaveBeenLastCalledWith("/pics/logo.png", true);
  });

  it("AC-70X5YX.6: a cancelled file dialog keeps the built-in and its mode lock", async () => {
    const h = harness();
    await h.state.useBuiltin("surf");
    h.deps.choose.mockResolvedValueOnce(null);
    await h.state.choose();
    expect(h.state.builtin).toBe("surf");
    expect(h.state.opaqueDisabledReason).not.toBeNull();
    expect(h.state.preview).toEqual(BUILTIN_PREVIEW);
  });

  it("AC-70X5YX.6: a file upload after a built-in sends the file", async () => {
    const h = harness();
    await h.state.useBuiltin("surf");
    await h.state.choose();
    h.state.askUpload();
    await h.state.confirm(UPLOAD_WORD);
    expect(h.deps.upload).toHaveBeenCalledTimes(1);
    expect(h.deps.upload.mock.calls[0]![0]).toBe("/pics/logo.png");
    expect(h.deps.uploadBuiltin).not.toHaveBeenCalled();
  });
});
