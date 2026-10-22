import { describe, expect, it } from "vitest";
import {
  ALL_FIT_CLASSES,
  chooseFit,
  COMPACT_LEVEL,
  fitClasses,
  MAX_FIT_LEVEL,
  moreMenuLabel,
  startLevel,
  type Fit,
} from "./topBarFit";

// That the bar really keeps one row with a wide font is checked in the browser by
// e2e/ui/layout.spec.ts (`E2E_WIDE_FONT=1`); this checks the order fits are tried in.
describe("top bar fit", () => {
  it("starts compact below 1000 px and full from 1000 px", () => {
    expect(startLevel(900)).toBe(COMPACT_LEVEL);
    expect(startLevel(999)).toBe(COMPACT_LEVEL);
    expect(startLevel(1000)).toBe(0);
  });

  it("takes the first level that fits, with the picker at full width", () => {
    const tried: Fit[] = [];
    const fit = chooseFit(0, (f) => {
      tried.push(f);
      return f.level >= 2;
    });
    expect(fit).toEqual({ level: 2, mode: "natural" });
    expect(tried.map((f) => f.level)).toEqual([0, 1, 2]);
  });

  it("never goes below the start level", () => {
    const tried: number[] = [];
    chooseFit(COMPACT_LEVEL, (f) => (tried.push(f.level), true));
    expect(tried).toEqual([COMPACT_LEVEL]);
  });

  it("shrinks the picker only after every level, and wraps only as the last resort", () => {
    expect(chooseFit(0, (f) => f.mode === "shrink")).toEqual({
      level: MAX_FIT_LEVEL,
      mode: "shrink",
    });
    expect(chooseFit(0, () => false)).toEqual({ level: MAX_FIT_LEVEL, mode: "wrap" });
  });

  it("names one class per level reached, plus the mode", () => {
    expect(fitClasses({ level: 0, mode: "natural" })).toEqual([]);
    expect(fitClasses({ level: 2, mode: "natural" })).toEqual(["hide-brand", "settings-in-menu"]);
    const all = fitClasses({ level: MAX_FIT_LEVEL, mode: "wrap" });
    expect(all).toContain("export-in-menu");
    expect(all).toContain("fit-wrap");
    for (const c of fitClasses({ level: MAX_FIT_LEVEL, mode: "shrink" })) {
      expect(ALL_FIT_CLASSES).toContain(c);
    }
  });

  it("names what the menu holds", () => {
    expect(moreMenuLabel(COMPACT_LEVEL)).toBe("More: theme and About");
    expect(moreMenuLabel(4)).toBe("More: sample rate, theme and About");
    expect(moreMenuLabel(5)).toBe("More: sample rate, Export, theme and About");
  });
});
