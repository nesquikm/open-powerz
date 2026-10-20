import { describe, expect, it } from "vitest";
import {
  THEME_COLORS,
  THEME_STORAGE_KEY,
  contrastRatio,
  loadTheme,
  motionSettings,
  nextTheme,
  prefersReducedMotion,
  resolveTheme,
  saveTheme,
  systemPrefersDark,
} from "./theme";

function memoryStorage(initial: Record<string, string> = {}) {
  const data = new Map(Object.entries(initial));
  return {
    getItem: (k: string) => data.get(k) ?? null,
    setItem: (k: string, v: string) => void data.set(k, v),
    data,
  };
}

function media(matches: Record<string, boolean>) {
  return (query: string) => ({ matches: matches[query] ?? false });
}

describe("theme choice (AC-AH2AEK.5)", () => {
  it("defaults to following the system", () => {
    expect(loadTheme(memoryStorage())).toBe("system");
  });

  it("resolves System from the OS preference", () => {
    expect(resolveTheme("system", true)).toBe("dark");
    expect(resolveTheme("system", false)).toBe("light");
    expect(resolveTheme("light", true)).toBe("light");
    expect(resolveTheme("dark", false)).toBe("dark");
  });

  it("reads the OS preference from matchMedia", () => {
    expect(systemPrefersDark(media({ "(prefers-color-scheme: dark)": true }))).toBe(true);
    expect(systemPrefersDark(media({}))).toBe(false);
  });

  it("cycles System, Light, Dark", () => {
    expect(nextTheme("system")).toBe("light");
    expect(nextTheme("light")).toBe("dark");
    expect(nextTheme("dark")).toBe("system");
  });

  it("persists the choice across restarts", () => {
    const storage = memoryStorage();
    saveTheme("dark", storage);
    expect(storage.data.get(THEME_STORAGE_KEY)).toBe("dark");
    expect(loadTheme(storage)).toBe("dark");
  });

  it("ignores a corrupt stored value", () => {
    expect(loadTheme(memoryStorage({ [THEME_STORAGE_KEY]: "neon" }))).toBe("system");
  });

  it("survives storage that throws", () => {
    const broken = {
      getItem: () => {
        throw new Error("denied");
      },
      setItem: () => {
        throw new Error("denied");
      },
    };
    expect(loadTheme(broken)).toBe("system");
    expect(() => saveTheme("light", broken)).not.toThrow();
    expect(loadTheme(broken)).toBe("system");
  });
});

describe("contrast (AC-AH2AEK.5)", () => {
  it("computes WCAG contrast ratios", () => {
    expect(contrastRatio("#000000", "#ffffff")).toBeCloseTo(21, 5);
    expect(contrastRatio("#ffffff", "#ffffff")).toBeCloseTo(1, 5);
    expect(contrastRatio("#777777", "#ffffff")).toBeCloseTo(4.48, 2);
  });

  for (const name of ["light", "dark"] as const) {
    const c = THEME_COLORS[name];
    it(`${name}: text reaches 4.5:1 on every background`, () => {
      for (const bg of [c.bg, c.surface]) {
        for (const fg of [c.text, c.muted, c.danger]) {
          expect(contrastRatio(fg, bg)).toBeGreaterThanOrEqual(4.5);
        }
      }
    });

    it(`${name}: controls, focus ring and chart lines reach 3:1`, () => {
      for (const bg of [c.bg, c.surface]) {
        for (const fg of [c.controlBorder, c.focus, c.accent]) {
          expect(contrastRatio(fg, bg)).toBeGreaterThanOrEqual(3);
        }
      }
      const series = Object.values(c.chart);
      expect(series.length).toBeGreaterThanOrEqual(7);
      for (const line of series) {
        expect(contrastRatio(line, c.surface)).toBeGreaterThanOrEqual(3);
        // The chart section sits on the page background too.
        expect(contrastRatio(line, c.bg)).toBeGreaterThanOrEqual(3);
      }
    });
  }
});

describe("reduced motion (AC-AH2AEK.7)", () => {
  it("reads the system preference", () => {
    expect(prefersReducedMotion(media({ "(prefers-reduced-motion: reduce)": true }))).toBe(true);
    expect(prefersReducedMotion(media({}))).toBe(false);
  });

  it("disables transitions and makes the chart jump", () => {
    expect(motionSettings(true)).toEqual({ transitions: false, chartScroll: "jump" });
    expect(motionSettings(false)).toEqual({ transitions: true, chartScroll: "smooth" });
  });
});
