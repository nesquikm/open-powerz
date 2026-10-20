/** Formats a voltage in volts for the meter readout, e.g. `5.000 V`. */
export function formatVolts(volts: number): string {
  return `${volts.toFixed(3)} V`;
}
