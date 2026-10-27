/** Records view logic (FR 1HHT00, AC .5): the offline-record list with its
 * decoded columns, download of one or all with progress, and a downloaded
 * record opened in its own chart with statistics. */

import { describe, expect, it, vi } from "vitest";
import type { OfflineRecord, OfflineRecordEntry, RecordList, UiError } from "./backend";
import {
  RecordsPanel,
  formatRecordDuration,
  recordChartData,
  recordRows,
  recordStats,
} from "./records.svelte";

const A: OfflineRecordEntry = {
  index: 1,
  name: "REC001",
  interval_s: 2,
  points: 3,
  duration_s: 4,
  charge_ah: 0.000299,
  energy_wh: 0.001495,
  offset: 0x1000,
};
const B: OfflineRecordEntry = {
  index: 2,
  name: "charger test",
  interval_s: 1,
  points: 2,
  duration_s: 3_725,
  charge_ah: 1.5,
  energy_wh: 7.5,
  offset: 0x10000,
};
const EMPTY: OfflineRecordEntry = {
  index: 3,
  name: "EMPTY",
  interval_s: 1,
  points: 0,
  duration_s: 0,
  charge_ah: 0,
  energy_wh: 0,
  offset: 0x20000,
};
const LIST: RecordList = { records: [A, B, EMPTY] };

/** Record A: three samples 2 s apart. */
const REC_A: OfflineRecord = {
  index: 1,
  name: "REC001",
  interval_s: 2,
  samples: [
    { t_s: 0, vbus_v: 5.0, ibus_a: 1.0, charge_ah: 0, energy_wh: 0 },
    { t_s: 2, vbus_v: 9.0, ibus_a: 2.0, charge_ah: 0.0001, energy_wh: 0.0009 },
    { t_s: 4, vbus_v: 7.0, ibus_a: -0.5, charge_ah: 0.000299, energy_wh: 0.001495 },
  ],
};
const REC_B: OfflineRecord = {
  index: 2,
  name: "charger test",
  interval_s: 1,
  samples: [
    { t_s: 0, vbus_v: 5, ibus_a: 0, charge_ah: 0, energy_wh: 0 },
    { t_s: 1, vbus_v: 5, ibus_a: 0, charge_ah: 1.5, energy_wh: 7.5 },
  ],
};
const BY_INDEX: Record<number, OfflineRecord> = { 1: REC_A, 2: REC_B };

const refused: UiError = { kind: "usage", message: "No meter is connected.", hint: null };

function backend() {
  const list = vi.fn(async () => LIST);
  const download = vi.fn(async (index: number) => {
    const r = BY_INDEX[index];
    if (!r) throw refused;
    return r;
  });
  return { list, download };
}

describe("formatRecordDuration (AC-1HHT00.5)", () => {
  it("formats seconds as h:mm:ss", () => {
    expect(formatRecordDuration(0)).toBe("0:00:00");
    expect(formatRecordDuration(4)).toBe("0:00:04");
    expect(formatRecordDuration(598)).toBe("0:09:58");
    expect(formatRecordDuration(3_725)).toBe("1:02:05");
    expect(formatRecordDuration(100 * 3600)).toBe("100:00:00");
  });
});

describe("recordRows (AC-1HHT00.5)", () => {
  it("has a row per record with every decoded column", () => {
    const rows = recordRows(LIST);
    expect(rows.map((r) => r.index)).toEqual([1, 2, 3]);
    const a = rows[0]!;
    expect(a.name).toBe("REC001");
    expect(a.interval).toBe("2 s");
    expect(a.points).toBe("3");
    expect(a.duration).toBe("0:00:04");
    expect(a.charge.endsWith(" Ah")).toBe(true);
    expect(parseFloat(a.charge)).toBeCloseTo(0.000299, 6);
    expect(a.energy.endsWith(" Wh")).toBe(true);
    expect(parseFloat(a.energy)).toBeCloseTo(0.001495, 6);
    expect(a.offset).toBe("0x00001000");
    expect(rows[1]!.duration).toBe("1:02:05");
    expect(rows[1]!.offset).toBe("0x00010000");
  });

  it("marks a record with no points as not downloadable", () => {
    const rows = recordRows(LIST);
    expect(rows.map((r) => r.downloadable)).toEqual([true, true, false]);
  });
});

describe("recordStats (AC-1HHT00.5)", () => {
  it("gives duration, totals and min/max/avg of V, I and power", () => {
    const s = recordStats(REC_A);
    expect(s.samples).toBe(3);
    expect(s.duration_s).toBe(4);
    expect(s.charge_ah).toBeCloseTo(0.000299, 9);
    expect(s.energy_wh).toBeCloseTo(0.001495, 9);
    expect(s.vbus).toEqual({ min: 5, max: 9, avg: 7 });
    expect(s.ibus!.min).toBeCloseTo(-0.5, 12);
    expect(s.ibus!.max).toBeCloseTo(2, 12);
    expect(s.ibus!.avg).toBeCloseTo(2.5 / 3, 12);
    expect(s.power!.min).toBeCloseTo(-3.5, 12);
    expect(s.power!.max).toBeCloseTo(18, 12);
    expect(s.power!.avg).toBeCloseTo((5 + 18 - 3.5) / 3, 12);
  });

  it("has no min/max/avg for a record without samples", () => {
    const s = recordStats({ index: 3, name: "EMPTY", interval_s: 1, samples: [] });
    expect(s.samples).toBe(0);
    expect(s.duration_s).toBe(0);
    expect(s.charge_ah).toBe(0);
    expect(s.energy_wh).toBe(0);
    expect(s.vbus).toBeNull();
    expect(s.ibus).toBeNull();
    expect(s.power).toBeNull();
  });
});

describe("recordChartData (AC-1HHT00.5)", () => {
  it("gives aligned time (s), VBUS, IBUS and power series", () => {
    const [t, v, i, p] = recordChartData(REC_A);
    expect(t).toEqual([0, 2, 4]);
    expect(v).toEqual([5, 9, 7]);
    expect(i).toEqual([1, 2, -0.5]);
    expect(p).toEqual([5, 18, -3.5]);
  });
});

describe("RecordsPanel (AC-1HHT00.5)", () => {
  it("starts empty and reads nothing by itself", () => {
    const { list, download } = backend();
    const p = new RecordsPanel(list, download);
    expect(p.records).toBeNull();
    expect(p.opened).toBeNull();
    expect(p.progress).toBeNull();
    expect(list).not.toHaveBeenCalled();
    expect(download).not.toHaveBeenCalled();
  });

  it("lists the records when the meter connects and on refresh", async () => {
    const { list, download } = backend();
    const p = new RecordsPanel(list, download);
    await p.connected();
    expect(p.records).toEqual(LIST.records);
    await p.refresh();
    expect(list).toHaveBeenCalledTimes(2);
    expect(download).not.toHaveBeenCalled();
  });

  it("shows a failed list as an error and no stale rows", async () => {
    const list = vi.fn<() => Promise<RecordList>>();
    list.mockResolvedValueOnce(LIST).mockRejectedValueOnce(refused);
    const p = new RecordsPanel(list, backend().download);
    await p.refresh();
    await p.refresh();
    expect(p.records).toBeNull();
    expect(p.error).toEqual(refused);
  });

  it("downloads one record and remembers it", async () => {
    const { list, download } = backend();
    const p = new RecordsPanel(list, download);
    await p.connected();
    expect(p.isDownloaded(2)).toBe(false);
    await p.download(2);
    expect(download).toHaveBeenCalledWith(2);
    expect(p.isDownloaded(2)).toBe(true);
    expect(p.isDownloaded(1)).toBe(false);
  });

  it("is busy with the record being downloaded", async () => {
    let resolve: (r: OfflineRecord) => void = () => {};
    const download = vi.fn(() => new Promise<OfflineRecord>((r) => (resolve = r)));
    const p = new RecordsPanel(backend().list, download);
    await p.connected();
    const done = p.download(1);
    expect(p.downloading).toBe(1);
    resolve(REC_A);
    await done;
    expect(p.downloading).toBeNull();
  });

  it("downloads all downloadable records in order with progress", async () => {
    const { list } = backend();
    const seen: Array<{ done: number; total: number } | null> = [];
    const p: RecordsPanel = new RecordsPanel(
      list,
      vi.fn(async (index: number) => {
        seen.push(p.progress ? { ...p.progress } : null);
        return BY_INDEX[index]!;
      }),
    );
    await p.connected();
    await p.downloadAll();
    // The 0-point record is skipped; progress counts the two others.
    expect(seen).toEqual([
      { done: 0, total: 2 },
      { done: 1, total: 2 },
    ]);
    expect(p.progress).toBeNull();
    expect(p.isDownloaded(1)).toBe(true);
    expect(p.isDownloaded(2)).toBe(true);
    expect(p.isDownloaded(3)).toBe(false);
  });

  it("a new download-all drops the previous run's done status", async () => {
    const download = vi.fn(async (index: number) => BY_INDEX[index]!);
    const p = new RecordsPanel(backend().list, download);
    await p.connected();
    await p.downloadAll();
    expect(p.lastAll).toBe(2);
    download.mockRejectedValueOnce(refused);
    await p.downloadAll();
    expect(p.lastAll).toBeNull();
    expect(p.error).toEqual(refused);
  });

  it("stops a download-all at the first failure and shows it", async () => {
    const download = vi.fn<(index: number) => Promise<OfflineRecord>>();
    download.mockRejectedValueOnce(refused);
    const p = new RecordsPanel(backend().list, download);
    await p.connected();
    await p.downloadAll();
    expect(download).toHaveBeenCalledTimes(1);
    expect(p.error).toEqual(refused);
    expect(p.progress).toBeNull();
    expect(p.isDownloaded(1)).toBe(false);
  });

  it("opens a downloaded record for the chart without downloading it again", async () => {
    const { list, download } = backend();
    const p = new RecordsPanel(list, download);
    await p.connected();
    await p.download(1);
    await p.open(1);
    expect(download).toHaveBeenCalledTimes(1);
    expect(p.opened).toEqual(REC_A);
    expect(p.openedStats).toEqual(recordStats(REC_A));
  });

  it("opening a record not downloaded yet downloads it first", async () => {
    const { list, download } = backend();
    const p = new RecordsPanel(list, download);
    await p.connected();
    await p.open(2);
    expect(download).toHaveBeenCalledWith(2);
    expect(p.opened).toEqual(REC_B);
    expect(p.isDownloaded(2)).toBe(true);
  });

  it("forgets the list on disconnect and drops a list in flight", async () => {
    let resolve: (l: RecordList) => void = () => {};
    const list = vi.fn(() => new Promise<RecordList>((r) => (resolve = r)));
    const p = new RecordsPanel(list, backend().download);
    const done = p.connected();
    p.disconnected();
    resolve(LIST);
    await done;
    expect(p.records).toBeNull();
    expect(p.loading).toBe(false);
  });
});
