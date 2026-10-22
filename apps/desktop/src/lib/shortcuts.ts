/** Keyboard shortcuts for the main window (AC-AH2AEK.6). */

/** The element a key event targets, reduced to what the text-input check needs. */
export interface TargetLike {
  tagName: string;
  type?: string;
  isContentEditable: boolean;
  /** The element's ARIA role, when it has one. */
  role?: string | null;
}

/** ARIA widgets that move through their items with the arrows, Home and End. */
const NAVIGATING_ROLES = new Set(["tab", "tablist", "grid", "row", "gridcell", "tree", "treeitem"]);

/** The parts of a KeyboardEvent the dispatcher reads. */
export interface KeyLike {
  key: string;
  ctrlKey: boolean;
  metaKey: boolean;
  altKey: boolean;
  shiftKey?: boolean;
  target: TargetLike | null;
}

export type ShortcutAction =
  | { kind: "record" }
  | { kind: "connect" }
  | { kind: "reset_stats" }
  | { kind: "rate"; sps: number }
  | { kind: "live" }
  | { kind: "home" }
  | { kind: "shuttle"; key: "J" | "K" | "L" }
  | { kind: "step"; direction: -1 | 1; coarse: boolean }
  | { kind: "zoom"; direction: "in" | "out" }
  | { kind: "fit" }
  | { kind: "theme" }
  | { kind: "help" };

/** Rows shown in the shortcut overlay, in display order. */
export const SHORTCUTS: readonly { keys: string; description: string }[] = [
  { keys: "Space", description: "Start or stop recording" },
  { keys: "C", description: "Connect or disconnect" },
  { keys: "R", description: "Reset statistics" },
  { keys: "1–4", description: "Sample rate: 2, 10, 50 or 1000 SPS" },
  { keys: "J / K / L", description: "Shuttle the playhead back / stop / forward" },
  { keys: "← / →", description: "Step the playhead (Shift: a tenth of the view)" },
  { keys: "+ / −", description: "Zoom in / out" },
  { keys: "Home / End", description: "Jump to the session start / return the chart to live" },
  { keys: "Shift+Z", description: "Fit the whole session" },
  { keys: "T", description: "Cycle theme (System, Light, Dark)" },
  { keys: "?", description: "Show this shortcut list" },
  { keys: "Esc", description: "Close the open menu, dialog or confirmation" },
];

/** Digit keys 1–4 to sample rates. */
const RATE_KEYS: Record<string, number> = { "1": 2, "2": 10, "3": 50, "4": 1000 };

/** Input types that do not take typed text, so shortcuts stay active on them. */
const NON_TEXT_INPUT_TYPES = new Set([
  "button",
  "checkbox",
  "color",
  "file",
  "hidden",
  "image",
  "radio",
  "range",
  "reset",
  "submit",
]);

/** True when the element accepts typed text (shortcuts must not fire there). */
export function isTextInput(target: TargetLike | null): boolean {
  if (target === null) return false;
  if (target.isContentEditable) return true;
  const tag = target.tagName.toUpperCase();
  if (tag === "TEXTAREA") return true;
  if (tag === "INPUT") return !NON_TEXT_INPUT_TYPES.has((target.type ?? "text").toLowerCase());
  return false;
}

/** True when the focused control uses `key` itself: a select's type-ahead takes
 * every printable key (and Home / End move its choice), a range input or an ARIA
 * widget (tabs, the PD table, the field tree) keeps ← / → / Home / End, and Space
 * activates a button or a toggle input. */
function controlOwnsKey(target: TargetLike | null, key: string): boolean {
  if (target === null) return false;
  const navigation = key === "ArrowLeft" || key === "ArrowRight" || key === "Home" || key === "End";
  if (navigation && NAVIGATING_ROLES.has(target.role ?? "")) return true;
  const tag = target.tagName.toUpperCase();
  const isRange = tag === "INPUT" && (target.type ?? "").toLowerCase() === "range";
  if (key === "ArrowLeft" || key === "ArrowRight") return tag === "SELECT" || isRange;
  if (isRange && (key === "Home" || key === "End")) return true;
  if (tag === "SELECT") return key.length === 1 || key === "Home" || key === "End";
  if (key !== " ") return false;
  return tag === "BUTTON" || (tag === "INPUT" && !isTextInput(target));
}

/** Map a key event to a shortcut action, or null when it is not one. */
export function shortcutFor(event: KeyLike): ShortcutAction | null {
  if (event.ctrlKey || event.metaKey || event.altKey) return null;
  if (isTextInput(event.target)) return null;
  const key = event.key;
  if (controlOwnsKey(event.target, key)) return null;
  if (key === " ") return { kind: "record" };
  if (key === "?") return { kind: "help" };
  if (key === "End") return { kind: "live" };
  if (key === "Home") return { kind: "home" };
  if (key === "ArrowLeft" || key === "ArrowRight") {
    return { kind: "step", direction: key === "ArrowLeft" ? -1 : 1, coarse: !!event.shiftKey };
  }
  if (key === "+" || key === "=") return { kind: "zoom", direction: "in" };
  if (key === "-" || key === "\u2212") return { kind: "zoom", direction: "out" };
  if ((key === "Z" || key === "z") && event.shiftKey) return { kind: "fit" };
  const sps = RATE_KEYS[key];
  if (sps !== undefined) return { kind: "rate", sps };
  switch (key.toLowerCase()) {
    case "c":
      return { kind: "connect" };
    case "r":
      return { kind: "reset_stats" };
    case "j":
      return { kind: "shuttle", key: "J" };
    case "k":
      return { kind: "shuttle", key: "K" };
    case "l":
      return { kind: "shuttle", key: "L" };
    case "t":
      return { kind: "theme" };
    default:
      return null;
  }
}
