/**
 * FR QFTCX0 (AC-QFTCX0.5): the browser recorder's CSV, in the desktop recorder's
 * format (`crates/powerz-record`): the vendor header, `h:mm:ss.mmm` elapsed time,
 * plain decimals with eight significant digits, and charge / energy by the
 * trapezoid rule from the first row.
 */

import type { Sample } from "../backend";

/** The CSV header line, matching the vendor recorder. */
export const CSV_HEADER = "ElapsedTime,Unix,VBUS,IBUS,DP,DM,CC1,CC2,TEMP,CHARGE,ENERGY,PWR";

const pad = (n: number, width: number): string => String(n).padStart(width, "0");

/** Elapsed milliseconds as `h:mm:ss.mmm` (hours unpadded). */
export function formatElapsed(elapsedMs: number): string {
  const total = Math.max(0, Math.floor(elapsedMs));
  const ms = total % 1000;
  const s = Math.floor(total / 1000);
  return `${Math.floor(s / 3600)}:${pad(Math.floor(s / 60) % 60, 2)}:${pad(s % 60, 2)}.${pad(ms, 3)}`;
}

/** `x` with at most 8 significant digits as a plain decimal, trailing zeros trimmed;
 * never exponent form. */
export function sig8(x: number): string {
  if (x === 0) return "0";
  if (!Number.isFinite(x)) return String(x);
  const mag = Math.floor(Math.log10(Math.abs(x)));
  if (mag >= 8) return x.toFixed(0);
  const s = x.toFixed(Math.min(100, Math.max(0, 7 - mag)));
  return s.includes(".") ? s.replace(/0+$/, "").replace(/\.$/, "") : s;
}

/** Collects recorded rows in memory under {@link CSV_HEADER}. Elapsed time and charge /
 * energy count from the first row; `Unix` is the wall clock given with the first row
 * plus the sample time elapsed since it. */
export class CsvRecorder {
  rows = 0;
  #lines: string[] = [CSV_HEADER];
  /** `[first row's t_ms, wall clock at it in ms]`. */
  #origin: [number, number] | null = null;
  /** The previous row `[t_ms, vbus_v, ibus_a]`, `null` at the start of a run. */
  #last: [number, number, number] | null = null;
  #chargeAh = 0;
  #energyWh = 0;

  push(sample: Sample, unixMs: number, tempC: number | null = null): void {
    const t = sample.t_ms;
    this.#origin ??= [t, unixMs];
    const [t0, unix0] = this.#origin;
    const elapsedMs = Math.max(0, Math.floor(t - t0));
    const v = sample.vbus_v;
    const i = sample.ibus_a;
    if (this.#last) {
      const [tl, vl, il] = this.#last;
      // A backwards step contributes nothing.
      const hours = Math.max(0, t - tl) / 3_600_000;
      this.#chargeAh += ((il + i) / 2) * hours;
      this.#energyWh += ((vl * il + v * i) / 2) * hours;
    }
    this.#last = [t, v, i];
    const unixTotal = Math.round(unix0) + elapsedMs;
    const unix = `${Math.floor(unixTotal / 1000)}.${pad(unixTotal % 1000, 3)}`;
    const temp = tempC === null || tempC === undefined ? "" : sig8(tempC);
    this.#lines.push(
      [
        formatElapsed(elapsedMs),
        unix,
        sig8(v),
        sig8(i),
        sig8(sample.dp_v),
        sig8(sample.dm_v),
        sig8(sample.cc1_v),
        sig8(sample.cc2_v),
        temp,
        sig8(this.#chargeAh),
        sig8(this.#energyWh),
        sig8(v * i),
      ].join(","),
    );
    this.rows++;
  }

  /** The interval up to the next row was not measured: it adds no charge or energy. */
  breakRun(): void {
    this.#last = null;
  }

  /** The CSV so far, one line per row, newline-terminated. */
  text(): string {
    return `${this.#lines.join("\n")}\n`;
  }
}
