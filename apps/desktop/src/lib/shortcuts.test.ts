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
    expect(shortcutFor(key("End"))).toEqual({ kind: "live" });
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
    expect(shortcutFor(key("l", { target: checkbox }))).toEqual({ kind: "shuttle", key: "L" });
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
    expect(keys).toEqual([
      "Space",
      "C",
      "R",
      "1–4",
      "J / K / L",
      "← / →",
      "+ / −",
      "Home / End",
      "Shift+Z",
      "T",
      "?",
      "Esc",
    ]);
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

  it("AC-DDDYDJ.5: the shortcut overlay lists End for going live (L is the shuttle now)", () => {
    const row = SHORTCUTS.find((s) => s.keys === "Home / End");
    expect(row?.description).toMatch(/live/i);
    expect(SHORTCUTS.find((s) => s.keys === "L / End")).toBeUndefined();
  });

  it("AC-DDDYDJ.5: a text field or a focused select keeps End for itself", () => {
    const select = { tagName: "SELECT", isContentEditable: false };
    const input = { tagName: "INPUT", isContentEditable: false };
    expect(shortcutFor(key("End", { target: select }))).toBeNull();
    expect(shortcutFor(key("End", { target: input }))).toBeNull();
  });
});

describe("timeline keys (FR FENCD9)", () => {
  const range = { tagName: "INPUT", type: "range", isContentEditable: false };
  const text = { tagName: "INPUT", type: "text", isContentEditable: false };

  it("AC-FENCD9.7: J / K / L drive the shuttle, in either case; L no longer goes live", () => {
    for (const [k, K] of [
      ["j", "J"],
      ["k", "K"],
      ["l", "L"],
    ] as const) {
      expect(shortcutFor(key(k))).toEqual({ kind: "shuttle", key: K });
      expect(shortcutFor(key(K, { shiftKey: true }))).toEqual({ kind: "shuttle", key: K });
    }
    expect(shortcutFor(key("l"))).not.toEqual({ kind: "live" });
  });

  it("AC-FENCD9.6: ← / → step the playhead; Shift makes it a coarse step", () => {
    expect(shortcutFor(key("ArrowLeft"))).toEqual({ kind: "step", direction: -1, coarse: false });
    expect(shortcutFor(key("ArrowRight"))).toEqual({ kind: "step", direction: 1, coarse: false });
    expect(shortcutFor(key("ArrowLeft", { shiftKey: true }))).toEqual({
      kind: "step",
      direction: -1,
      coarse: true,
    });
    expect(shortcutFor(key("ArrowRight", { shiftKey: true }))).toEqual({
      kind: "step",
      direction: 1,
      coarse: true,
    });
  });

  it("AC-FENCD9.8: + or = zoom in, − zooms out", () => {
    expect(shortcutFor(key("+", { shiftKey: true }))).toEqual({ kind: "zoom", direction: "in" });
    expect(shortcutFor(key("="))).toEqual({ kind: "zoom", direction: "in" });
    expect(shortcutFor(key("-"))).toEqual({ kind: "zoom", direction: "out" });
  });

  it("AC-FENCD9.8: Home goes to the session start, End goes live, Shift+Z fits the session", () => {
    expect(shortcutFor(key("Home"))).toEqual({ kind: "home" });
    expect(shortcutFor(key("End"))).toEqual({ kind: "live" });
    expect(shortcutFor(key("Z", { shiftKey: true }))).toEqual({ kind: "fit" });
  });

  it("AC-FENCD9.13: a focused range input keeps ←, →, Home and End; other shortcuts still work", () => {
    for (const k of ["ArrowLeft", "ArrowRight", "Home", "End"]) {
      expect(shortcutFor(key(k, { target: range }))).toBeNull();
      expect(shortcutFor(key(k, { target: range, shiftKey: true }))).toBeNull();
    }
    expect(shortcutFor(key("l", { target: range }))).toEqual({ kind: "shuttle", key: "L" });
  });

  it("AC-FENCD9.13: a text field keeps every new key", () => {
    for (const k of ["j", "k", "l", "ArrowLeft", "ArrowRight", "+", "=", "-", "Home", "Z"]) {
      expect(shortcutFor(key(k, { target: text }))).toBeNull();
    }
  });

  it("AC-FENCD9.13: the overlay describes the new keys", () => {
    const row = (keys: string) => SHORTCUTS.find((s) => s.keys === keys)?.description ?? "";
    expect(row("J / K / L")).toMatch(/shuttle/i);
    expect(row("J / K / L")).toMatch(/back/i);
    expect(row("J / K / L")).toMatch(/stop/i);
    expect(row("J / K / L")).toMatch(/forward/i);
    expect(row("← / →")).toMatch(/playhead/i);
    expect(row("← / →")).toMatch(/Shift/);
    expect(row("← / →")).toMatch(/tenth of the view/i);
    expect(row("+ / −")).toMatch(/zoom in/i);
    expect(row("+ / −")).toMatch(/out/i);
    expect(row("Home / End")).toMatch(/session start/i);
    expect(row("Home / End")).toMatch(/live/i);
    expect(row("Shift+Z")).toMatch(/whole session/i);
  });
});

describe("the overlay lists Escape (FR M46CTX)", () => {
  it("AC-M46CTX.4: a row for Escape says it closes or cancels what is open", () => {
    const row = SHORTCUTS.find((s) => /^Esc(ape)?$/.test(s.keys));
    expect(row, "an Esc / Escape row").toBeDefined();
    expect(row?.description ?? "").toMatch(/close|cancel/i);
  });
});

describe("ARIA widgets keep their own navigation keys (FR TGE83C / TGE83D)", () => {
  it("a focused tab, grid row or tree item keeps the arrows, Home and End", () => {
    for (const role of ["tab", "grid", "row", "tree", "treeitem"]) {
      const target = { tagName: "DIV", isContentEditable: false, role };
      for (const k of ["ArrowLeft", "ArrowRight", "Home", "End"]) {
        expect(shortcutFor(key(k, { target })), `${role} ${k}`).toBeNull();
      }
      // Other shortcuts still work there.
      expect(shortcutFor(key("?", { target }))).toEqual({ kind: "help" });
    }
  });
});
