/** Records view delete logic (FR JKWT3V, AC .3 and .4): the panel sends the typed
 * confirmation, shows the new list and the backup folder on success, and on a
 * failure shows the error once without trying again. */

import { describe, expect, it, vi } from "vitest";
import type {
  OfflineRecord,
  OfflineRecordEntry,
  RecordDeleteResult,
  RecordList,
  UiError,
} from "./backend";
import { RecordsPanel } from "./records.svelte";

function entry(index: number, name: string, points = 2): OfflineRecordEntry {
  return {
    index,
    name,
    interval_s: 1,
    points,
    duration_s: points - 1,
    charge_ah: 0.1,
    energy_wh: 0.5,
    offset: index * 0x1000,
  };
}

const A = entry(1, "REC001");
const B = entry(2, "charger test");
const C = entry(3, "LAST");
const LIST: RecordList = { records: [A, B, C] };

const REC_A: OfflineRecord = {
  index: 1,
  name: "REC001",
  interval_s: 1,
  samples: [{ t_s: 0, vbus_v: 5, ibus_a: 1, charge_ah: 0, energy_wh: 0 }],
};

const mismatch: UiError = {
  kind: "protocol",
  message: "The record list does not match after the delete. Backup: /bk/records-1",
  hint: null,
};

function harness(result?: RecordDeleteResult, fail?: UiError) {
  const list = vi.fn(async () => LIST);
  const download = vi.fn(async () => REC_A);
  const remove = vi.fn<(target: number | "all", confirm: string) => Promise<RecordDeleteResult>>(
    async () => {
      if (fail) throw fail;
      return result!;
    },
  );
  const panel = new RecordsPanel(list, download, remove);
  return { list, download, remove, panel };
}

describe("RecordsPanel delete (AC-JKWT3V.3, .4)", () => {
  it("deletes one record with the typed name and shows the new list and the backup", async () => {
    const result: RecordDeleteResult = {
      records: [A, { ...C, index: 2 }],
      backup_dir: "/bk/records-1",
      backups: ["/bk/records-1/02_charger_test.csv"],
    };
    const { panel, remove } = harness(result);
    await panel.connected();
    await panel.delete(2, "charger test");
    expect(remove).toHaveBeenCalledTimes(1);
    expect(remove).toHaveBeenCalledWith(2, "charger test");
    expect(panel.records?.map((r) => r.name)).toEqual(["REC001", "LAST"]);
    expect(panel.lastDelete).toEqual(result);
    expect(panel.error).toBeNull();
    expect(panel.deleting).toBeNull();
  });

  it("deletes all with the word all", async () => {
    const result: RecordDeleteResult = { records: [], backup_dir: "/bk/records-2", backups: [] };
    const { panel, remove } = harness(result);
    await panel.connected();
    await panel.delete("all", "all");
    expect(remove).toHaveBeenCalledWith("all", "all");
    expect(panel.records).toEqual([]);
  });

  it("marks the delete in flight", async () => {
    let release: (r: RecordDeleteResult) => void = () => {};
    const list = vi.fn(async () => LIST);
    const remove = vi.fn(
      () =>
        new Promise<RecordDeleteResult>((resolve) => {
          release = resolve;
        }),
    );
    const panel = new RecordsPanel(list, vi.fn(), remove);
    await panel.connected();
    const done = panel.delete(3, "LAST");
    expect(panel.deleting).toBe(3);
    release({ records: [A, B], backup_dir: "/bk", backups: [] });
    await done;
    expect(panel.deleting).toBeNull();
  });

  it("shows a failed delete as the error, with its backup path, and does not retry", async () => {
    const { panel, remove } = harness(undefined, mismatch);
    await panel.connected();
    await panel.delete(2, "charger test");
    expect(panel.error).toEqual(mismatch);
    expect(panel.error?.message).toContain("/bk/records-1");
    expect(remove).toHaveBeenCalledTimes(1);
    expect(panel.lastDelete).toBeNull();
    expect(panel.deleting).toBeNull();
  });

  it("after a failed delete it lists again, so the rows match the meter, and keeps the error", async () => {
    const { panel, list } = harness(undefined, mismatch);
    await panel.connected();
    list.mockResolvedValueOnce({ records: [A, C] });
    await panel.delete(2, "charger test");
    expect(list).toHaveBeenCalledTimes(2);
    expect(panel.records).toEqual([A, C]);
    expect(panel.error).toEqual(mismatch);
  });

  it("when that list fails too, no stale rows stay and the delete error still shows", async () => {
    const { panel, list } = harness(undefined, mismatch);
    await panel.connected();
    list.mockRejectedValueOnce({ kind: "device", message: "gone", hint: null });
    await panel.delete(2, "charger test");
    expect(panel.records).toBeNull();
    expect(panel.error).toEqual(mismatch);
  });

  it("forgets a download whose record moved or went away", async () => {
    const result: RecordDeleteResult = {
      records: [A, { ...C, index: 2 }],
      backup_dir: "/bk",
      backups: [],
    };
    const { panel } = harness(result);
    await panel.connected();
    await panel.download(1);
    await panel.download(2);
    await panel.delete(2, "charger test");
    expect(panel.isDownloaded(1)).toBe(true);
    expect(panel.isDownloaded(2)).toBe(false);
  });

  it("a disconnect forgets the last delete", async () => {
    const { panel } = harness({ records: [A], backup_dir: "/bk", backups: [] });
    await panel.connected();
    await panel.delete(2, "charger test");
    panel.disconnected();
    expect(panel.lastDelete).toBeNull();
  });
});
