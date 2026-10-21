/**
 * How the top bar keeps one row with any system font: it tries its overflow
 * levels in order, each moving more lower-priority content out of the row
 * (into the "⋯" menu or screen-reader-only text), with the device picker at
 * its natural width; only when even the last level does not fit does the
 * picker shrink (ellipsis, full label in its tooltip), and only when that
 * fails too does the bar wrap.
 */

/** Overflow levels; a level includes every level below it. */
export const FIT_LEVELS = [
  /** 0: everything inline. */
  null,
  /** 1: the wordmark goes (the window title names the app). */
  "hide-brand",
  /** 2: theme, About and the rate-locked note move into the "⋯" menu. */
  "settings-in-menu",
  /** 3: tighter spacing; the recording indicator shows just its time and a running
   * export just its progress bar in place of the (disabled) Export button. */
  "compact",
  /** 4: the sample-rate picker moves into the menu. */
  "rate-in-menu",
  /** 5: the Export button moves into the menu. */
  "export-in-menu",
] as const;

export type FitLevel = number;

export const SETTINGS_IN_MENU_LEVEL = 2;
export const COMPACT_LEVEL = 3;
export const RATE_IN_MENU_LEVEL = 4;
export const EXPORT_IN_MENU_LEVEL = 5;
export const MAX_FIT_LEVEL = FIT_LEVELS.length - 1;

/** Below this window width (CSS px) the bar starts compact, whatever fits. */
export const COMPACT_BELOW_PX = 1000;

/** `natural`: picker at full width; `shrink`: picker may shrink; `wrap`: the bar may wrap. */
export type FitMode = "natural" | "shrink" | "wrap";

export interface Fit {
  level: FitLevel;
  mode: FitMode;
}

/** The level the search starts at: narrow windows always get the compact bar, so
 * the layout does not jump between states there. */
export function startLevel(viewportWidth: number): FitLevel {
  return viewportWidth < COMPACT_BELOW_PX ? COMPACT_LEVEL : 0;
}

/** The first fit, in priority order, for which `fits` says the bar is one row. */
export function chooseFit(start: FitLevel, fits: (fit: Fit) => boolean): Fit {
  for (let level = Math.max(0, start); level <= MAX_FIT_LEVEL; level++) {
    if (fits({ level, mode: "natural" })) return { level, mode: "natural" };
  }
  const shrink: Fit = { level: MAX_FIT_LEVEL, mode: "shrink" };
  if (fits(shrink)) return shrink;
  return { level: MAX_FIT_LEVEL, mode: "wrap" };
}

/** The classes the bar carries for `fit` (one per level reached, plus the mode). */
export function fitClasses(fit: Fit): string[] {
  const classes: string[] = [];
  for (let level = 1; level <= fit.level && level <= MAX_FIT_LEVEL; level++) {
    const name = FIT_LEVELS[level];
    if (name) classes.push(name);
  }
  if (fit.mode !== "natural") classes.push(`fit-${fit.mode}`);
  return classes;
}

/** Every class `fitClasses` can return, to clear before applying a new fit. */
export const ALL_FIT_CLASSES: readonly string[] = [
  ...FIT_LEVELS.filter((n): n is NonNullable<typeof n> => n !== null),
  "fit-shrink",
  "fit-wrap",
];

/** The accessible name of the "⋯" button: what the menu holds at `level`. */
export function moreMenuLabel(level: FitLevel): string {
  const extra = [
    level >= RATE_IN_MENU_LEVEL ? "sample rate" : null,
    level >= EXPORT_IN_MENU_LEVEL ? "Export" : null,
  ].filter((s) => s !== null);
  return extra.length === 0
    ? "More: theme and About"
    : `More: ${extra.join(", ")}, theme and About`;
}
