// FR CW66T1, AC .8: the "Show (i) icons" switch is on by default and saved across restarts.
import { describe, expect, it } from "vitest";
import { loadShowInfoIcons, saveShowInfoIcons, SHOW_INFO_ICONS_STORAGE_KEY } from "./settings";
import type { ThemeStorage } from "./theme";

function memory(
  initial: Record<string, string> = {},
): ThemeStorage & { data: Record<string, string> } {
  const data = { ...initial };
  return {
    data,
    getItem: (k: string) => data[k] ?? null,
    setItem: (k: string, v: string) => {
      data[k] = v;
    },
  } as ThemeStorage & { data: Record<string, string> };
}

const throwing = {
  getItem: () => {
    throw new Error("denied");
  },
  setItem: () => {
    throw new Error("denied");
  },
} as unknown as ThemeStorage;

describe("AC-CW66T1.8: Show (i) icons setting", () => {
  it("is saved under its own key", () => {
    expect(SHOW_INFO_ICONS_STORAGE_KEY).toBe("open-powerz.show-info-icons");
  });

  it("is on by default: no storage, nothing saved, or storage that throws", () => {
    expect(loadShowInfoIcons(undefined)).toBe(true);
    expect(loadShowInfoIcons(memory())).toBe(true);
    expect(loadShowInfoIcons(throwing)).toBe(true);
  });

  it("an off switch is remembered, and so is turning it back on", () => {
    const s = memory();
    saveShowInfoIcons(false, s);
    expect(s.data[SHOW_INFO_ICONS_STORAGE_KEY]).toBe("false");
    expect(loadShowInfoIcons(s)).toBe(false);
    saveShowInfoIcons(true, s);
    expect(s.data[SHOW_INFO_ICONS_STORAGE_KEY]).toBe("true");
    expect(loadShowInfoIcons(s)).toBe(true);
  });

  it('only a saved "false" turns it off', () => {
    expect(loadShowInfoIcons(memory({ [SHOW_INFO_ICONS_STORAGE_KEY]: "garbage" }))).toBe(true);
    expect(loadShowInfoIcons(memory({ [SHOW_INFO_ICONS_STORAGE_KEY]: "false" }))).toBe(false);
  });

  it("a storage that throws on save keeps the app running", () => {
    let error: unknown = null;
    try {
      saveShowInfoIcons(false, throwing);
    } catch (e) {
      error = e;
    }
    expect(error).toBeNull();
  });
});
