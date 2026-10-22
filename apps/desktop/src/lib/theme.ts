import type { PdMarkerKind } from "./pdTimeline";

/** User theme choice; `system` follows `prefers-color-scheme`. */
export type ThemeChoice = "system" | "light" | "dark";
/** Theme actually painted. */
export type ResolvedTheme = "light" | "dark";

/** localStorage key holding the persisted {@link ThemeChoice}. */
export const THEME_STORAGE_KEY = "open-powerz.theme";

/** Minimal storage surface (localStorage-compatible); calls may throw. */
export interface ThemeStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

/** Minimal `window.matchMedia` surface. */
export type MatchMedia = (query: string) => { matches: boolean };

/** Media query for the OS dark color scheme. */
export const DARK_QUERY = "(prefers-color-scheme: dark)";
/** Media query for the OS reduced-motion setting. */
export const REDUCED_MOTION_QUERY = "(prefers-reduced-motion: reduce)";
const CHOICES: readonly ThemeChoice[] = ["system", "light", "dark"];

function isChoice(value: unknown): value is ThemeChoice {
  return typeof value === "string" && (CHOICES as readonly string[]).includes(value);
}

/** Read the persisted choice; missing, corrupt or unreadable storage gives `system`. */
export function loadTheme(storage: ThemeStorage | undefined): ThemeChoice {
  try {
    const value = storage?.getItem(THEME_STORAGE_KEY);
    return isChoice(value) ? value : "system";
  } catch {
    return "system";
  }
}

/** Persist the choice; storage failures are ignored (the choice just won't survive a restart). */
export function saveTheme(choice: ThemeChoice, storage: ThemeStorage | undefined): void {
  try {
    storage?.setItem(THEME_STORAGE_KEY, choice);
  } catch {
    // storage unavailable (private mode, quota, denied): keep the in-memory choice
  }
}

/** Toggle order: System → Light → Dark → System. */
export function nextTheme(choice: ThemeChoice): ThemeChoice {
  return CHOICES[(CHOICES.indexOf(choice) + 1) % CHOICES.length] ?? "system";
}

/** Resolve a choice against the OS dark-mode preference. */
export function resolveTheme(choice: ThemeChoice, systemDark: boolean): ResolvedTheme {
  if (choice === "system") return systemDark ? "dark" : "light";
  return choice;
}

/** Whether the OS asks for a dark color scheme. */
export function systemPrefersDark(matchMedia: MatchMedia | undefined): boolean {
  return matchMedia?.(DARK_QUERY).matches ?? false;
}

/** Palette for one theme. Text roles ≥ 4.5:1, UI roles and chart lines ≥ 3:1 (WCAG 2.2 AA). */
export interface ThemePalette {
  bg: string;
  surface: string;
  text: string;
  muted: string;
  danger: string;
  /** Decorative separators (not a control boundary, no contrast requirement). */
  border: string;
  controlBorder: string;
  focus: string;
  accent: string;
  chart: Record<string, string>;
  /** PD marker colours on the power chart, one per marker kind (FR TGE83D). */
  pdMarker: Record<PdMarkerKind, string>;
}

export const THEME_COLORS: Record<ResolvedTheme, ThemePalette> = {
  light: {
    bg: "#ffffff",
    surface: "#f5f6f8",
    text: "#16181d",
    muted: "#525966",
    danger: "#b42318",
    border: "#d9dce2",
    controlBorder: "#6b7280",
    focus: "#1d4ed8",
    accent: "#1d4ed8",
    chart: {
      voltage: "#1d4ed8",
      current: "#b42318",
      power: "#047857",
      dPlus: "#7c3aed",
      dMinus: "#b45309",
      cc1: "#0e7490",
      cc2: "#be185d",
    },
    pdMarker: {
      negotiation: "#0f766e",
      reset: "#c2410c",
      error: "#b91c1c",
      attach_detach: "#6d28d9",
      other: "#4b5563",
    },
  },
  dark: {
    bg: "#111317",
    surface: "#1a1d23",
    text: "#e8eaee",
    muted: "#a3aab6",
    danger: "#ff7b72",
    border: "#2a2e36",
    controlBorder: "#7d8590",
    focus: "#6ea8fe",
    accent: "#6ea8fe",
    chart: {
      voltage: "#6ea8fe",
      current: "#ff7b72",
      power: "#4ade80",
      dPlus: "#c4a5ff",
      dMinus: "#fbbf24",
      cc1: "#22d3ee",
      cc2: "#f472b6",
    },
    pdMarker: {
      negotiation: "#2dd4bf",
      reset: "#fb923c",
      error: "#f87171",
      attach_detach: "#a78bfa",
      other: "#9ca3af",
    },
  },
};

function channel(c: number): number {
  const s = c / 255;
  return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
}

function luminance(hex: string): number {
  const h = hex.replace("#", "");
  const full =
    h.length === 3
      ? h
          .split("")
          .map((x) => x + x)
          .join("")
      : h;
  const n = Number.parseInt(full, 16);
  return (
    0.2126 * channel((n >> 16) & 0xff) +
    0.7152 * channel((n >> 8) & 0xff) +
    0.0722 * channel(n & 0xff)
  );
}

/** WCAG 2.x contrast ratio between two `#rrggbb` / `#rgb` colors (1–21). */
export function contrastRatio(a: string, b: string): number {
  const la = luminance(a);
  const lb = luminance(b);
  return (Math.max(la, lb) + 0.05) / (Math.min(la, lb) + 0.05);
}

function kebab(name: string): string {
  return name.replace(/[A-Z]/g, (m) => `-${m.toLowerCase()}`);
}

/** CSS custom properties for a theme, generated from {@link THEME_COLORS} so CSS and tests can't drift. */
export function themeVariables(theme: ResolvedTheme): Record<string, string> {
  const p = THEME_COLORS[theme];
  const vars: Record<string, string> = {};
  for (const [key, value] of Object.entries(p)) {
    if (typeof value === "string") vars[`--${kebab(key)}`] = value;
  }
  for (const [key, value] of Object.entries(p.chart)) {
    vars[`--chart-${kebab(key)}`] = value;
  }
  for (const [key, value] of Object.entries(p.pdMarker)) {
    vars[`--pd-${key.replace(/_/g, "-")}`] = value;
  }
  return vars;
}

/** Paint a resolved theme on `<html>`: `data-theme`, `color-scheme` and the palette variables. */
export function applyTheme(theme: ResolvedTheme, root: HTMLElement): void {
  root.dataset.theme = theme;
  root.style.colorScheme = theme;
  for (const [name, value] of Object.entries(themeVariables(theme))) {
    root.style.setProperty(name, value);
  }
}

/** `window.localStorage`, or `undefined` where merely touching it throws (sandboxed / denied). */
export function browserStorage(): ThemeStorage | undefined {
  try {
    return window.localStorage;
  } catch {
    return undefined;
  }
}

/** How the UI animates: CSS transitions on/off, and how the chart follows new data. */
export interface MotionSettings {
  transitions: boolean;
  chartScroll: "jump" | "smooth";
}

/** Whether the OS asks for reduced motion. */
export function prefersReducedMotion(matchMedia: MatchMedia | undefined): boolean {
  return matchMedia?.(REDUCED_MOTION_QUERY).matches ?? false;
}

/** Reduced motion disables transitions and makes the chart jump to new data. */
export function motionSettings(reduced: boolean): MotionSettings {
  return reduced
    ? { transitions: false, chartScroll: "jump" }
    : { transitions: true, chartScroll: "smooth" };
}

/**
 * Calls `onChange` with the new match state whenever `query` flips; returns the
 * unsubscribe function (a no-op where `window.matchMedia` is unavailable).
 */
export function watchMediaQuery(query: string, onChange: (matches: boolean) => void): () => void {
  const list = window.matchMedia?.(query);
  if (!list) return () => {};
  const listener = (e: MediaQueryListEvent) => onChange(e.matches);
  list.addEventListener("change", listener);
  return () => list.removeEventListener("change", listener);
}
