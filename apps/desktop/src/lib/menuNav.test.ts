import { describe, expect, it } from "vitest";
import { menuIndexAfter } from "./menuNav";

describe("menuIndexAfter: roving focus in the ⋯ menu (AC-M46CTX.2)", () => {
  it("ArrowDown moves to the next item and wraps from the last to the first", () => {
    expect(menuIndexAfter("ArrowDown", 0, 4)).toBe(1);
    expect(menuIndexAfter("ArrowDown", 2, 4)).toBe(3);
    expect(menuIndexAfter("ArrowDown", 3, 4)).toBe(0);
  });

  it("ArrowUp moves to the previous item and wraps from the first to the last", () => {
    expect(menuIndexAfter("ArrowUp", 3, 4)).toBe(2);
    expect(menuIndexAfter("ArrowUp", 1, 4)).toBe(0);
    expect(menuIndexAfter("ArrowUp", 0, 4)).toBe(3);
  });

  it("Home goes to the first item, End to the last", () => {
    expect(menuIndexAfter("Home", 2, 4)).toBe(0);
    expect(menuIndexAfter("End", 0, 4)).toBe(3);
    expect(menuIndexAfter("End", 3, 4)).toBe(3);
  });

  it("starts from the first (ArrowDown) or the last (ArrowUp) item when none has focus", () => {
    expect(menuIndexAfter("ArrowDown", -1, 4)).toBe(0);
    expect(menuIndexAfter("ArrowUp", -1, 4)).toBe(3);
  });

  it("ignores keys that do not move focus", () => {
    for (const key of ["Enter", " ", "Escape", "Tab", "a", "ArrowLeft", "ArrowRight"]) {
      expect(menuIndexAfter(key, 1, 4)).toBeNull();
    }
  });

  it("does nothing in an empty menu", () => {
    for (const key of ["ArrowDown", "ArrowUp", "Home", "End"]) {
      expect(menuIndexAfter(key, -1, 0)).toBeNull();
    }
  });

  it("a one-item menu stays on its item", () => {
    for (const key of ["ArrowDown", "ArrowUp", "Home", "End"]) {
      expect(menuIndexAfter(key, 0, 1)).toBe(0);
    }
  });
});
