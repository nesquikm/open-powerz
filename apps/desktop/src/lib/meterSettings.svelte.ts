/**
 * The read-only Meter settings panel's state (FR 1HHT01): the connected meter's
 * settings, each with its confidence tag, read on connect and on Refresh and
 * forgotten on disconnect.
 */

import { toUiError, type MeterSettings, type SettingTag, type UiError } from "./backend";

/** One displayed row: the field with its tag worded. */
export interface MeterSettingRow {
  key: string;
  label: string;
  value: string;
  tag: SettingTag;
  tagText: string;
}

/** The tag as the panel words it. */
export function tagText(tag: SettingTag): string {
  return tag === "confirmed" ? "confirmed" : "inferred";
}

/** Every field in order, with its tag text. */
export function meterSettingRows(settings: MeterSettings): MeterSettingRow[] {
  return settings.fields.map((f) => ({ ...f, tagText: tagText(f.tag) }));
}

/** The panel's settings, read through `read` (the backend's `meterSettings`). */
export class MeterSettingsPanel {
  settings = $state<MeterSettings | null>(null);
  error = $state<UiError | null>(null);
  loading = $state(false);
  readonly #read: () => Promise<MeterSettings>;
  /** Bumped on disconnect and on each read, so a stale answer is dropped. */
  #generation = 0;

  constructor(read: () => Promise<MeterSettings>) {
    this.#read = read;
  }

  /** A meter connected: read its settings. */
  connected(): Promise<void> {
    return this.refresh();
  }

  /** Read the settings again; a failed read shows the error and no stale values. */
  async refresh(): Promise<void> {
    const generation = ++this.#generation;
    this.loading = true;
    try {
      const settings = await this.#read();
      if (generation !== this.#generation) return;
      this.settings = settings;
      this.error = null;
    } catch (e) {
      if (generation !== this.#generation) return;
      this.settings = null;
      this.error = toUiError(e);
    } finally {
      if (generation === this.#generation) this.loading = false;
    }
  }

  /** The meter went away: forget its settings and drop any read in flight. */
  disconnected(): void {
    this.#generation += 1;
    this.settings = null;
    this.error = null;
    this.loading = false;
  }
}
