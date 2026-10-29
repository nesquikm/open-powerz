/**
 * The Records view's state (FR 1HHT00, AC .5): the meter's offline records with
 * their decoded columns, read on connect and on Refresh; download of one or all
 * with progress; and a downloaded record opened in its own chart with its
 * statistics. The live session's history is never touched.
 */

import { SvelteMap } from "svelte/reactivity";
import {
  toUiError,
  type OfflineRecord,
  type OfflineRecordEntry,
  type RecordDeleteResult,
  type RecordList,
  type UiError,
} from "./backend";

/** `s` seconds as h:mm:ss (hours not wrapped). */
export function formatRecordDuration(s: number): string {
  const total = Math.max(0, Math.round(s));
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const sec = total % 60;
  return `${h}:${String(m).padStart(2, "0")}:${String(sec).padStart(2, "0")}`;
}

/** A charge or energy value: more decimals below 1, so small records still read. */
function formatAmount(value: number, unit: string): string {
  return `${value.toFixed(Math.abs(value) >= 1 ? 4 : 6)} ${unit}`;
}

/** One displayed row of the record list. */
export interface RecordRow {
  index: number;
  name: string;
  interval: string;
  points: string;
  duration: string;
  charge: string;
  energy: string;
  offset: string;
  /** The record has samples to download. */
  downloadable: boolean;
}

/** Every record of `list` as a table row. Charge and energy show as magnitudes, as the
 * meter's screen shows them: the meter stores them signed (negative for one current
 * direction). Downloaded samples keep their sign (raw measurements). */
export function recordRows(list: RecordList): RecordRow[] {
  return list.records.map((r) => ({
    index: r.index,
    name: r.name,
    interval: `${r.interval_s} s`,
    points: String(r.points),
    duration: formatRecordDuration(r.duration_s),
    charge: formatAmount(Math.abs(r.charge_ah), "Ah"),
    energy: formatAmount(Math.abs(r.energy_wh), "Wh"),
    offset: `0x${r.offset.toString(16).toUpperCase().padStart(8, "0")}`,
    downloadable: r.points > 0,
  }));
}

/** Min, max and average of one channel. */
export interface RecordMinMaxAvg {
  min: number;
  max: number;
  avg: number;
}

/** Statistics of a downloaded record. */
export interface RecordStats {
  samples: number;
  duration_s: number;
  charge_ah: number;
  energy_wh: number;
  vbus: RecordMinMaxAvg | null;
  ibus: RecordMinMaxAvg | null;
  power: RecordMinMaxAvg | null;
}

function minMaxAvg(values: readonly number[]): RecordMinMaxAvg | null {
  if (values.length === 0) return null;
  let min = Infinity;
  let max = -Infinity;
  let sum = 0;
  for (const v of values) {
    if (v < min) min = v;
    if (v > max) max = v;
    sum += v;
  }
  return { min, max, avg: sum / values.length };
}

/** Duration, totals (the last sample's) and min/max/avg of V, I and power. */
export function recordStats(rec: OfflineRecord): RecordStats {
  const last = rec.samples[rec.samples.length - 1];
  return {
    samples: rec.samples.length,
    duration_s: last?.t_s ?? 0,
    charge_ah: last?.charge_ah ?? 0,
    energy_wh: last?.energy_wh ?? 0,
    vbus: minMaxAvg(rec.samples.map((s) => s.vbus_v)),
    ibus: minMaxAvg(rec.samples.map((s) => s.ibus_a)),
    power: minMaxAvg(rec.samples.map((s) => s.vbus_v * s.ibus_a)),
  };
}

/** Aligned series for the record chart: time (s), VBUS, IBUS and power. */
export function recordChartData(rec: OfflineRecord): [number[], number[], number[], number[]] {
  return [
    rec.samples.map((s) => s.t_s),
    rec.samples.map((s) => s.vbus_v),
    rec.samples.map((s) => s.ibus_a),
    rec.samples.map((s) => s.vbus_v * s.ibus_a),
  ];
}

/** Download-all progress: records done of the total. */
export interface RecordProgress {
  done: number;
  total: number;
}

/** The Records view's list, downloads and opened record. */
export class RecordsPanel {
  records = $state<OfflineRecordEntry[] | null>(null);
  error = $state<UiError | null>(null);
  loading = $state(false);
  /** The index being downloaded, or null. */
  downloading = $state<number | null>(null);
  progress = $state<RecordProgress | null>(null);
  /** How many records the last finished download-all fetched, or null. */
  lastAll = $state<number | null>(null);
  opened = $state<OfflineRecord | null>(null);
  openedStats = $derived(this.opened ? recordStats(this.opened) : null);
  /** The delete in flight: a record index, `"all"`, or null (FR JKWT3V). */
  deleting = $state<number | "all" | null>(null);
  /** The last finished delete: the new list and the backup folder, or null. */
  lastDelete = $state<RecordDeleteResult | null>(null);
  /** Downloaded records by index. */
  readonly #downloaded = new SvelteMap<number, OfflineRecord>();
  readonly #list: () => Promise<RecordList>;
  readonly #download: (index: number) => Promise<OfflineRecord>;
  readonly #delete: (target: number | "all", confirm: string) => Promise<RecordDeleteResult>;
  /** Bumped on disconnect and on each list, so a stale answer is dropped. */
  #generation = 0;

  constructor(
    list: () => Promise<RecordList>,
    download: (index: number) => Promise<OfflineRecord>,
    remove: (target: number | "all", confirm: string) => Promise<RecordDeleteResult> = () =>
      Promise.reject(new Error("Deleting records is not available.")),
  ) {
    this.#list = list;
    this.#download = download;
    this.#delete = remove;
  }

  /** A meter connected: list its records. */
  connected(): Promise<void> {
    return this.refresh();
  }

  /** List the records again; a failed list shows the error and no stale rows. */
  async refresh(): Promise<void> {
    const generation = ++this.#generation;
    this.loading = true;
    try {
      const { records } = await this.#list();
      if (generation !== this.#generation) return;
      this.#showList(records);
    } catch (e) {
      if (generation !== this.#generation) return;
      this.records = null;
      this.error = toUiError(e);
    } finally {
      if (generation === this.#generation) this.loading = false;
    }
  }

  /** Show `records` as the list. A record downloaded before stays only while the
   * meter still lists it at the same place. */
  #showList(records: OfflineRecordEntry[]): void {
    this.records = records;
    this.error = null;
    for (const [i, r] of [...this.#downloaded]) {
      if (records[i - 1]?.name !== r.name) this.#downloaded.delete(i);
    }
  }

  /** Delete record `target` (1-based), or every record with `"all"`, sending the typed
   * `confirm` (FR JKWT3V). On success the new list and the backup show; a failure
   * shows the error (with its backup path) and is not retried. */
  async delete(target: number | "all", confirm: string): Promise<void> {
    const generation = this.#generation;
    this.deleting = target;
    this.lastDelete = null;
    try {
      const result = await this.#delete(target, confirm);
      if (generation !== this.#generation) return;
      this.#showList(result.records);
      this.lastDelete = result;
    } catch (e) {
      if (generation === this.#generation) this.error = toUiError(e);
    } finally {
      if (generation === this.#generation) this.deleting = null;
    }
  }

  /** Record `index` has been downloaded. */
  isDownloaded(index: number): boolean {
    return this.#downloaded.has(index);
  }

  /** Download one record; a failure shows the error. Resolves to the record, or
   * null when it failed or the meter went away. */
  async download(index: number): Promise<OfflineRecord | null> {
    const generation = this.#generation;
    this.downloading = index;
    try {
      const rec = await this.#download(index);
      if (generation !== this.#generation) return null;
      this.#downloaded.set(index, rec);
      this.error = null;
      return rec;
    } catch (e) {
      if (generation === this.#generation) this.error = toUiError(e);
      return null;
    } finally {
      if (generation === this.#generation) this.downloading = null;
    }
  }

  /** Download every record with samples, in order, with progress; stops at the
   * first failure. */
  async downloadAll(): Promise<void> {
    const todo = (this.records ?? []).filter((r) => r.points > 0);
    const generation = this.#generation;
    this.lastAll = null;
    this.progress = { done: 0, total: todo.length };
    try {
      for (const [k, r] of todo.entries()) {
        this.progress = { done: k, total: todo.length };
        if ((await this.download(r.index)) === null) return;
        if (generation !== this.#generation) return;
      }
      this.lastAll = todo.length;
    } finally {
      if (generation === this.#generation) this.progress = null;
    }
  }

  /** Open record `index` in the chart, downloading it first when needed. */
  async open(index: number): Promise<void> {
    const rec = this.#downloaded.get(index) ?? (await this.download(index));
    if (rec) this.opened = rec;
  }

  /** Close the opened record. */
  close(): void {
    this.opened = null;
  }

  /** The meter went away: forget its list and downloads and drop any answer in
   * flight. An opened record stays open. */
  disconnected(): void {
    this.#generation += 1;
    this.records = null;
    this.error = null;
    this.loading = false;
    this.downloading = null;
    this.progress = null;
    this.lastAll = null;
    this.deleting = null;
    this.lastDelete = null;
    this.#downloaded.clear();
  }
}
