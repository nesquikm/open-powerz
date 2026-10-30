/**
 * The Meter settings panel's state (FR 1HHT01): the connected meter's settings, each
 * with its confidence tag, read on connect and on Refresh and forgotten on
 * disconnect. Edit (FR JKWT3W) drafts a change of the writable fields, summarises
 * it, and Apply sends it once and shows the block as read back.
 */

import {
  toUiError,
  type EditableSettingKey,
  type MeterSettings,
  type SettingChanges,
  type SettingTag,
  type UiError,
} from "./backend";

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

/** One choice of a listed setting: its wire code and how the panel shows it. */
export interface SettingOption {
  code: number;
  label: string;
}

/** A writable setting: a list of choices, or a whole number in `min..max`. */
export interface EditableSetting {
  key: EditableSettingKey;
  label: string;
  options?: readonly SettingOption[];
  min?: number;
  max?: number;
}

function listed(labels: readonly string[]): SettingOption[] {
  return labels.map((label, code) => ({ code, label }));
}

/** The writable fields in display order, each with its valid values (FR JKWT3W). */
export const EDITABLE_SETTINGS: readonly EditableSetting[] = [
  { key: "language", label: "Language", options: listed(["Chinese", "English"]) },
  { key: "brightness", label: "Brightness", min: 1, max: 100 },
  {
    key: "dim_timeout",
    label: "Dim screen after",
    options: listed(["off", "15 s", "30 s", "60 s"]),
  },
  {
    key: "theme",
    label: "Theme",
    options: listed(["blue", "green", "pink", "orange", "yellow", "dark green", "cyan"]),
  },
  { key: "rotation", label: "Rotation", options: listed(["0°", "90°", "180°", "270°"]) },
  {
    key: "sample_rate",
    label: "Sample rate",
    options: listed(["2 SPS", "10 SPS", "50 SPS", "1000 SPS"]),
  },
];

function editable(key: string): EditableSetting | undefined {
  return EDITABLE_SETTINGS.find((s) => s.key === key);
}

/** Whether `code` is a valid value of `setting`. */
export function validSettingCode(setting: EditableSetting, code: number): boolean {
  if (!Number.isInteger(code)) return false;
  if (setting.options) return setting.options.some((o) => o.code === code);
  return code >= (setting.min ?? 0) && code <= (setting.max ?? 0);
}

/** How `setting` shows `code`, e.g. `80%` or `pink`. */
export function settingValueText(setting: EditableSetting, code: number): string {
  if (setting.options) return setting.options.find((o) => o.code === code)?.label ?? String(code);
  return `${code}%`;
}

/** The current code of every writable field, read from the displayed rows; a field
 * whose value is missing or unknown is left out. */
export function settingCodes(settings: MeterSettings): SettingChanges {
  const codes: SettingChanges = {};
  for (const s of EDITABLE_SETTINGS) {
    const value = settings.fields.find((f) => f.key === s.key)?.value;
    if (value === undefined) continue;
    const code = s.options
      ? s.options.find((o) => o.label === value)?.code
      : Number.parseInt(value, 10);
    if (code !== undefined && validSettingCode(s, code)) codes[s.key] = code;
  }
  return codes;
}

/** Each change worded as `Label: old → new`, in display order. */
export function changeSummary(settings: MeterSettings, changes: SettingChanges): string[] {
  const lines: string[] = [];
  for (const s of EDITABLE_SETTINGS) {
    const code = changes[s.key];
    if (code === undefined) continue;
    const row = settings.fields.find((f) => f.key === s.key);
    lines.push(`${row?.label ?? s.label}: ${row?.value ?? "?"} → ${settingValueText(s, code)}`);
  }
  return lines;
}

/** Shown after a write the meter read back as asked: the KM003C stores it at once but
 * redraws a new brightness or theme only after that setting is opened in its own menu
 * (2026-10-29) [device]. */
export const SAVED_NOTICE =
  "The meter has saved the change; its screen shows it after you open that setting in the meter's menu.";

/** The panel's settings, read through `read` (the backend's `meterSettings`) and
 * changed through `write` (the backend's `meterSettingsWrite`). */
export class MeterSettingsPanel {
  settings = $state<MeterSettings | null>(null);
  error = $state<UiError | null>(null);
  loading = $state(false);
  /** The edit form is open. */
  editing = $state(false);
  /** The edited code of every writable field. */
  draft = $state<SettingChanges>({});
  /** A write is in flight. */
  applying = $state(false);
  /** Set to SAVED_NOTICE after a successful write; cleared by the next edit. */
  notice = $state<string | null>(null);
  readonly #read: () => Promise<MeterSettings>;
  readonly #write: (changes: SettingChanges) => Promise<MeterSettings>;
  /** Bumped on disconnect and on each read, so a stale answer is dropped. */
  #generation = 0;

  constructor(
    read: () => Promise<MeterSettings>,
    write: (changes: SettingChanges) => Promise<MeterSettings> = () =>
      Promise.reject(toUiError({ kind: "usage", message: "Settings cannot be changed here." })),
  ) {
    this.#read = read;
    this.#write = write;
  }

  /** The drafted fields whose code differs from the settings as read. */
  get changes(): SettingChanges {
    if (!this.editing || !this.settings) return {};
    const current = settingCodes(this.settings);
    const changes: SettingChanges = {};
    for (const s of EDITABLE_SETTINGS) {
      const code = this.draft[s.key];
      if (code !== undefined && code !== current[s.key]) changes[s.key] = code;
    }
    return changes;
  }

  /** The change worded for the summary shown before Apply. */
  get summary(): string[] {
    return this.settings ? changeSummary(this.settings, this.changes) : [];
  }

  /** Open the edit form with the values just read; nothing to edit before a read. */
  startEdit(): void {
    if (!this.settings || this.applying) return;
    this.draft = settingCodes(this.settings);
    this.error = null;
    this.notice = null;
    this.editing = true;
  }

  /** Draft `code` for `key`; a value outside the field's range is refused (false). */
  setDraft(key: EditableSettingKey, code: number): boolean {
    const s = editable(key);
    if (!this.editing || !s || !validSettingCode(s, code)) return false;
    this.draft = { ...this.draft, [key]: code };
    return true;
  }

  /** Close the edit form without writing. */
  cancelEdit(): void {
    if (this.applying) return;
    this.editing = false;
    this.draft = {};
    this.error = null;
  }

  /** Send the change once; on success show the block as read back, on a failure show
   * the error and keep the draft. Nothing is retried. */
  async apply(): Promise<void> {
    const changes = this.changes;
    if (this.applying || Object.keys(changes).length === 0) return;
    const generation = this.#generation;
    this.applying = true;
    try {
      const after = await this.#write(changes);
      if (generation !== this.#generation) return;
      this.settings = after;
      this.error = null;
      this.notice = SAVED_NOTICE;
      this.editing = false;
      this.draft = {};
    } catch (e) {
      if (generation !== this.#generation) return;
      this.error = toUiError(e);
    } finally {
      if (generation === this.#generation) this.applying = false;
    }
  }

  /** A meter connected: read its settings. */
  connected(): Promise<void> {
    return this.refresh();
  }

  /** Read the settings again; a failed read shows the error and no stale values.
   * Ignored while a write is in flight, whose answer is the fresher read. */
  async refresh(): Promise<void> {
    if (this.applying) return;
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
    this.notice = null;
    this.loading = false;
    this.editing = false;
    this.draft = {};
    this.applying = false;
  }
}
