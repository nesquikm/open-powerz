import type { DeviceListing, Sample } from "./backend";

/** Shown in place of a value while there is none (e.g. disconnected). */
export const DASH = "—";

/** The true minus sign for displayed negatives; files and clipboard keep ASCII "-". */
export const MINUS = "\u2212";

/** A count with en-US thousands separators, whatever the page's locale. */
export function formatCount(n: number): string {
  return n.toLocaleString("en-US");
}

/** Formats `value` with `decimals` places, or {@link DASH} when missing. Never renders "−0.000". */
export function formatFixed(value: number | null | undefined, decimals: number): string {
  if (value === null || value === undefined || !Number.isFinite(value)) return DASH;
  const text = value.toFixed(decimals);
  if (Number(text) === 0) return (0).toFixed(decimals);
  return text.startsWith("-") ? MINUS + text.slice(1) : text;
}

/** Signed current with an arrow for its direction (→ positive, ← negative, none at zero). */
export function formatCurrent(amps: number | null | undefined): { text: string; arrow: string } {
  const text = formatFixed(amps, 3);
  if (text === DASH || Number(text) === 0) return { text, arrow: "" };
  return { text, arrow: (amps ?? 0) < 0 ? "←" : "→" };
}

/** What the IBUS arrow means, for its tooltip and screen readers; null when there is no arrow. */
export function currentDirectionText(amps: number | null | undefined): string | null {
  const { arrow } = formatCurrent(amps);
  if (arrow === "→") return "Current flows source → sink";
  if (arrow === "←") return "Current flows sink → source (reversed)";
  return null;
}

/**
 * Temperature text with one decimal, plus a tooltip: at 1000 SPS the value is
 * the latest once-a-second snapshot; a missing value is explained.
 */
export function temperatureDisplay(
  tempC: number | null,
  rateSps: number | null,
): { text: string; tooltip: string | null } {
  if (tempC !== null && Number.isFinite(tempC))
    return {
      text: formatFixed(tempC, 1),
      tooltip: rateSps === 1000 ? "Updated once a second at 1000 SPS" : null,
    };
  return {
    text: DASH,
    tooltip:
      rateSps === 1000 ? "Not reported by the meter at 1000 SPS" : "Not reported by the meter",
  };
}

/** Every string the live readout shows. */
export interface ReadoutView {
  vbus: string;
  ibus: string;
  ibusArrow: string;
  power: string;
  temp: string;
  tempTooltip: string | null;
  dp: string;
  dm: string;
  cc1: string;
  cc2: string;
}

/** Formats the latest sample for the readout; every field is {@link DASH} when `sample` is null. */
export function readoutView(
  sample: Sample | null,
  tempC: number | null,
  rateSps: number | null,
): ReadoutView {
  const current = formatCurrent(sample?.ibus_a);
  const temp = sample
    ? temperatureDisplay(tempC, rateSps)
    : { text: DASH, tooltip: null as string | null };
  return {
    vbus: formatFixed(sample?.vbus_v, 3),
    ibus: current.text,
    ibusArrow: current.arrow,
    power: formatFixed(sample ? sample.vbus_v * sample.ibus_a : null, 3),
    temp: temp.text,
    tempTooltip: temp.tooltip,
    dp: formatFixed(sample?.dp_v, 3),
    dm: formatFixed(sample?.dm_v, 3),
    cc1: formatFixed(sample?.cc1_v, 3),
    cc2: formatFixed(sample?.cc2_v, 3),
  };
}

/** What the page shows for `serial`: every place that shows one comes through here, so
 * the Hide serial numbers switch covers them all (FR AXHK2Q, AC .2, .3); with the switch
 * on it reads "***" (AC-CCF47K.3). */
export function displaySerial(serial: string, hidden: boolean): string {
  return hidden ? "***" : serial;
}

/** A backend-made "<model> — <serial>" label as the page shows it: the backend sends it
 * whole, so with the option on only the model before the dash stays, and a label with no
 * model part reads "***" (AC-AXHK2Q.2, AC-CCF47K.3). */
export function displayMeterLabel(label: string, hidden: boolean): string {
  if (!hidden) return label;
  const dash = label.indexOf(" — ");
  return dash < 0 ? displaySerial(label, hidden) : label.slice(0, dash);
}

/** Picker label: "Demo device — <scenario>" for demo entries, "<model> — <serial>" for
 * meters, with "(bootloader mode)" after a meter in bootloader mode. With `hidden`, a
 * meter is named by its model alone (FR CCF47K, AC .1); when `among` lists more than one
 * real meter of its model, " (n)" follows the model (AC .2). */
export function deviceOptionLabel(
  device: DeviceListing,
  hidden: boolean,
  among: readonly DeviceListing[] = [],
): string {
  if (device.demo_scenario !== null) return `Demo device — ${device.demo_scenario}`;
  let name = device.serial && !hidden ? `${device.model} — ${device.serial}` : device.model;
  if (hidden) {
    const n = sameModelNumber(device, among);
    if (n !== null) name = `${name} (${n})`;
  }
  // A meter in bootloader mode is listed as such (FR V9SK52, AC .5).
  return device.mode === "bootloader" ? `${name} (bootloader mode)` : name;
}

/** `device`'s 1-based place among the real meters of its model in `among`: by serial
 * (numeric collation), meters without one after them by id. Null when the model is
 * listed once (AC-CCF47K.2). */
function sameModelNumber(device: DeviceListing, among: readonly DeviceListing[]): number | null {
  const peers = among.filter((d) => d.demo_scenario === null && d.model === device.model);
  if (!peers.some((d) => d.id === device.id)) peers.push(device);
  if (peers.length < 2) return null;
  const collator = new Intl.Collator("en", { numeric: true });
  peers.sort((a, b) => {
    if (a.serial && b.serial)
      return collator.compare(a.serial, b.serial) || collator.compare(a.id, b.id);
    if (a.serial) return -1;
    if (b.serial) return 1;
    return collator.compare(a.id, b.id);
  });
  return peers.findIndex((d) => d.id === device.id) + 1;
}

/** Real meters first, then demo devices; order within each group is kept. */
export function sortDevices(devices: readonly DeviceListing[]): DeviceListing[] {
  const isDemo = (d: DeviceListing) => (d.demo_scenario === null ? 0 : 1);
  return [...devices].sort((a, b) => isDemo(a) - isDemo(b));
}

/** Connection state shown by the top-bar badge. */
export type ConnectionState =
  "disconnected" | "connecting" | "streaming" | "rate_locked" | "error" | "waiting";

const BADGE_LABELS: Record<ConnectionState, string> = {
  disconnected: "Disconnected",
  connecting: "Connecting",
  streaming: "Streaming",
  rate_locked: "Rate locked",
  error: "Error",
  waiting: "Waiting for meter",
};

/** Human label for a connection state. */
export function badgeLabel(state: ConnectionState): string {
  return BADGE_LABELS[state];
}
