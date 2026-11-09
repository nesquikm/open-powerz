/**
 * FR JFRPME: the web history's temperature log, as the desktop's `TempLog`
 * (`src-tauri/src/store.rs`): one entry per temperature change.
 */

/** `[first history index, °C, last session time it applies to (ms)]`. */
export type TempEntry = [number, number, number];

/** Temperature changes over a session history, sorted by index. */
export class TempLog {
  entries: TempEntry[] = [];

  /** The reading `tempC`, read at session time `readMs` and current until `untilMs`,
   * applies from history index `index` on. The same temperature read again before the
   * last entry expired extends that entry instead of adding one. */
  record(index: number, tempC: number, readMs: number, untilMs: number): void {
    const last = this.entries[this.entries.length - 1];
    if (last && last[1] === tempC && readMs <= last[2]) {
      last[2] = Math.max(last[2], untilMs);
    } else {
      this.entries.push([index, tempC, untilMs]);
    }
  }

  /** The temperature at history index `index` (session time `tMs`), moving `cursor`
   * forward; indexes must be asked for in increasing order. `null` before the first
   * reading or once the current one expired. */
  at(index: number, tMs: number, cursor: { i: number }): number | null {
    const e = this.entries;
    while (cursor.i + 1 < e.length && (e[cursor.i + 1] as TempEntry)[0] <= index) cursor.i++;
    const cur = e[cursor.i];
    return cur && cur[0] <= index && tMs <= cur[2] ? cur[1] : null;
  }

  /** Drop every entry. */
  clear(): void {
    this.entries = [];
  }
}
