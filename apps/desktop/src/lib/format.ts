import type { DeviceListing, Sample } from "./backend";

/** Shown in place of a value while there is none (e.g. disconnected). */
export const DASH = "—";

/** Formats `value` with `decimals` places, or {@link DASH} when missing. Never renders "-0.000". */
export function formatFixed(value: number | null | undefined, decimals: number): string {
  if (value === null || value === undefined || !Number.isFinite(value)) return DASH;
  const text = value.toFixed(decimals);
  return Number(text) === 0 ? (0).toFixed(decimals) : text;
}

/** Signed current with an arrow for its direction (→ positive, ← negative, none at zero). */
export function formatCurrent(amps: number | null | undefined): { text: string; arrow: string } {
  const text = formatFixed(amps, 3);
  if (text === DASH || Number(text) === 0) return { text, arrow: "" };
  return { text, arrow: text.startsWith("-") ? "←" : "→" };
}

/** Temperature text with one decimal, plus a tooltip explaining a missing value. */
export function temperatureDisplay(
  tempC: number | null,
  rateSps: number | null,
): { text: string; tooltip: string | null } {
  if (tempC !== null && Number.isFinite(tempC))
    return { text: formatFixed(tempC, 1), tooltip: null };
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

/** Picker label: "Demo device — <scenario>" for demo entries, "<model> — <serial>" for meters. */
export function deviceOptionLabel(device: DeviceListing): string {
  if (device.demo_scenario !== null) return `Demo device — ${device.demo_scenario}`;
  return device.serial ? `${device.model} — ${device.serial}` : device.model;
}

/** Real meters first, then demo devices; order within each group is kept. */
export function sortDevices(devices: readonly DeviceListing[]): DeviceListing[] {
  const isDemo = (d: DeviceListing) => (d.demo_scenario === null ? 0 : 1);
  return [...devices].sort((a, b) => isDemo(a) - isDemo(b));
}

/** Connection state shown by the top-bar badge. */
export type ConnectionState = "disconnected" | "connecting" | "streaming" | "rate_locked" | "error";

const BADGE_LABELS: Record<ConnectionState, string> = {
  disconnected: "Disconnected",
  connecting: "Connecting",
  streaming: "Streaming",
  rate_locked: "Rate locked",
  error: "Error",
};

/** Human label for a connection state. */
export function badgeLabel(state: ConnectionState): string {
  return BADGE_LABELS[state];
}
