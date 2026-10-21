import { describe, expect, it } from "vitest";
import { SHORTCUTS, isTextInput, shortcutFor, type KeyLike } from "./shortcuts";

function key(k: string, extra: Partial<KeyLike> = {}): KeyLike {
  return { key: k, ctrlKey: false, metaKey: false, altKey: false, target: null, ...extra };
}

describe("shortcutFor (AC-AH2AEK.6)", () => {
  it("maps every documented key", () => {
    expect(shortcutFor(key(" "))).toEqual({ kind: "record" });
    expect(shortcutFor(key("c"))).toEqual({ kind: "connect" });
    expect(shortcutFor(key("r"))).toEqual({ kind: "reset_stats" });
    expect(shortcutFor(key("l"))).toEqual({ kind: "live" });
    expect(shortcutFor(key("t"))).toEqual({ kind: "theme" });
    expect(shortcutFor(key("?", { shiftKey: true }))).toEqual({ kind: "help" });
  });

  it("selects 2/10/50/1000 SPS with 1–4", () => {
    expect(shortcutFor(key("1"))).toEqual({ kind: "rate", sps: 2 });
    expect(shortcutFor(key("2"))).toEqual({ kind: "rate", sps: 10 });
    expect(shortcutFor(key("3"))).toEqual({ kind: "rate", sps: 50 });
    expect(shortcutFor(key("4"))).toEqual({ kind: "rate", sps: 1000 });
    expect(shortcutFor(key("5"))).toBeNull();
  });

  it("accepts upper-case letters (Caps Lock)", () => {
    expect(shortcutFor(key("C"))).toEqual({ kind: "connect" });
    expect(shortcutFor(key("T"))).toEqual({ kind: "theme" });
  });

  it("leaves modifier combinations to the system", () => {
    expect(shortcutFor(key("c", { metaKey: true }))).toBeNull();
    expect(shortcutFor(key("c", { ctrlKey: true }))).toBeNull();
    expect(shortcutFor(key("r", { altKey: true }))).toBeNull();
  });

  it("ignores unmapped keys", () => {
    expect(shortcutFor(key("x"))).toBeNull();
    expect(shortcutFor(key("Enter"))).toBeNull();
  });

  it("is ignored while a text input has focus", () => {
    const text = { tagName: "INPUT", type: "text", isContentEditable: false };
    expect(shortcutFor(key("c", { target: text }))).toBeNull();
    expect(shortcutFor(key(" ", { target: text }))).toBeNull();
    expect(
      shortcutFor(key("1", { target: { tagName: "TEXTAREA", isContentEditable: false } })),
    ).toBeNull();
    expect(
      shortcutFor(key("t", { target: { tagName: "DIV", isContentEditable: true } })),
    ).toBeNull();
  });

  it("still works while a button or checkbox has focus", () => {
    const button = { tagName: "BUTTON", isContentEditable: false };
    const checkbox = { tagName: "INPUT", type: "checkbox", isContentEditable: false };
    expect(shortcutFor(key("r", { target: button }))).toEqual({ kind: "reset_stats" });
    expect(shortcutFor(key("l", { target: checkbox }))).toEqual({ kind: "live" });
  });
});

describe("isTextInput (AC-AH2AEK.6)", () => {
  it("recognizes text-entry elements", () => {
    expect(isTextInput({ tagName: "INPUT", type: "search", isContentEditable: false })).toBe(true);
    expect(isTextInput({ tagName: "INPUT", type: "number", isContentEditable: false })).toBe(true);
    expect(isTextInput({ tagName: "INPUT", isContentEditable: false })).toBe(true);
    expect(isTextInput({ tagName: "INPUT", type: "range", isContentEditable: false })).toBe(false);
    expect(isTextInput({ tagName: "SELECT", isContentEditable: false })).toBe(false);
    expect(isTextInput(null)).toBe(false);
  });
});

describe("SHORTCUTS overlay list (AC-AH2AEK.6)", () => {
  it("documents every shortcut", () => {
    const keys = SHORTCUTS.map((s) => s.keys);
    expect(keys).toEqual(["Space", "C", "R", "1–4", "L / End", "T", "?"]);
    for (const s of SHORTCUTS) {
      expect(s.description.length).toBeGreaterThan(0);
    }
  });
});

describe("focused controls keep their own keys (review round 1)", () => {
  const base = { ctrlKey: false, metaKey: false, altKey: false };
  const on = (tagName: string, type?: string) => ({ tagName, type, isContentEditable: false });

  it("lets Space activate a focused button or checkbox instead of recording", () => {
    expect(shortcutFor({ ...base, key: " ", target: on("BUTTON") })).toBeNull();
    expect(shortcutFor({ ...base, key: " ", target: on("INPUT", "checkbox") })).toBeNull();
    // Other shortcuts still work while a button has focus.
    expect(shortcutFor({ ...base, key: "c", target: on("BUTTON") })).toEqual({ kind: "connect" });
  });

  it("leaves every printable key to a focused select's type-ahead", () => {
    for (const key of [" ", "c", "1", "4", "t", "?"]) {
      expect(shortcutFor({ ...base, key, target: on("SELECT") })).toBeNull();
    }
  });

  it("still records with Space when nothing in particular has focus", () => {
    expect(shortcutFor({ ...base, key: " ", target: on("BODY") })).toEqual({ kind: "record" });
  });
});

describe("End returns the chart to live (FR DDDYDJ)", () => {
  it("AC-DDDYDJ.5: End anywhere outside a text field or a select returns to live", () => {
    expect(shortcutFor(key("End"))).toEqual({ kind: "live" });
    const button = { tagName: "BUTTON", isContentEditable: false };
    expect(shortcutFor(key("End", { target: button }))).toEqual({ kind: "live" });
  });

  it("AC-DDDYDJ.5: the shortcut overlay lists End beside L", () => {
    expect(SHORTCUTS).toContainEqual({ keys: "L / End", description: "Return the chart to live" });
  });

  it("AC-DDDYDJ.5: a text field or a focused select keeps End for itself", () => {
    const select = { tagName: "SELECT", isContentEditable: false };
    const input = { tagName: "INPUT", isContentEditable: false };
    expect(shortcutFor(key("End", { target: select }))).toBeNull();
    expect(shortcutFor(key("End", { target: input }))).toBeNull();
  });
});
