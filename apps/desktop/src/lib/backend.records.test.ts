/** Offline records over Tauri IPC (FR 1HHT00, AC .5). */

import { beforeEach, describe, expect, it, vi } from "vitest";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => {
  class Channel {
    onmessage: (msg: unknown) => void = () => {};
  }
  return { invoke, Channel, isTauri: () => true };
});

import { recordDownload, recordsList, type OfflineRecord, type RecordList } from "./backend";

beforeEach(() => {
  invoke.mockReset();
});

describe("recordsList (AC-1HHT00.5)", () => {
  it("reads the list through the records_list command", async () => {
    const reply: RecordList = {
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
    };
    invoke.mockResolvedValue(reply);
    await expect(recordsList()).resolves.toEqual(reply);
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(invoke).toHaveBeenCalledWith("records_list");
  });

  it("passes a refused list on as the backend error", async () => {
    const err = { kind: "usage", message: "No meter is connected.", hint: null };
    invoke.mockRejectedValue(err);
    await expect(recordsList()).rejects.toEqual(err);
  });
});

describe("recordDownload (AC-1HHT00.5)", () => {
  it("downloads one record by its 1-based index through record_download", async () => {
    const reply: OfflineRecord = {
      index: 2,
      name: "REC002",
      interval_s: 1,
      samples: [{ t_s: 0, vbus_v: 5, ibus_a: 1, charge_ah: 0, energy_wh: 0 }],
    };
    invoke.mockResolvedValue(reply);
    await expect(recordDownload(2)).resolves.toEqual(reply);
    expect(invoke).toHaveBeenCalledWith("record_download", { index: 2 });
  });
});
