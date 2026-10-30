/** Deleting offline records over Tauri IPC (FR JKWT3V, AC .3 and .4): the typed
 * confirmation travels with the request, so the backend checks it too. */

import { beforeEach, describe, expect, it, vi } from "vitest";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => {
  class Channel {
    onmessage: (msg: unknown) => void = () => {};
  }
  return { invoke, Channel, isTauri: () => true };
});

import { recordsDelete, type RecordDeleteResult } from "./backend";

beforeEach(() => {
  invoke.mockReset();
});

const RESULT: RecordDeleteResult = {
  records: [
    {
      index: 1,
      name: "REC001",
      interval_s: 1,
      points: 2,
      duration_s: 1,
      charge_ah: 0.1,
      energy_wh: 0.5,
      offset: 0,
    },
  ],
  backup_dir: "/tmp/open-powerz/backups/records-1",
  backups: ["/tmp/open-powerz/backups/records-1/02_charger_test.csv"],
};

describe("recordsDelete (AC-JKWT3V.3)", () => {
  it("deletes one record by its 1-based index with the typed name", async () => {
    invoke.mockResolvedValue(RESULT);
    await expect(recordsDelete(2, "charger test")).resolves.toEqual(RESULT);
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(invoke).toHaveBeenCalledWith("records_delete", { index: 2, confirm: "charger test" });
  });

  it("deletes every record with index null and the typed word all", async () => {
    invoke.mockResolvedValue({ ...RESULT, records: [] });
    await recordsDelete("all", "all");
    expect(invoke).toHaveBeenCalledWith("records_delete", { index: null, confirm: "all" });
  });

  it("passes a refused delete on as the backend error, with the backup path (AC-JKWT3V.4)", async () => {
    const err = {
      kind: "protocol",
      message:
        "The meter's record list does not match after the delete. Backup: /tmp/open-powerz/backups/records-1",
      hint: null,
    };
    invoke.mockRejectedValue(err);
    await expect(recordsDelete(1, "REC001")).rejects.toEqual(err);
    expect(invoke).toHaveBeenCalledTimes(1);
  });
});
