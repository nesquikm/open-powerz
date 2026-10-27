/**
 * FR 1HHT03: several meters at once. `meterBackend(deviceId)` is the backend of one
 * meter's tile: every command it sends names that meter (`deviceId`), so an action
 * never reaches another meter. The plain functions keep their single-meter arguments.
 */

import { beforeEach, describe, expect, it, vi } from "vitest";

const { invoke, channels } = vi.hoisted(() => ({
  invoke: vi.fn(),
  channels: [] as { onmessage: (msg: unknown) => void }[],
}));

vi.mock("@tauri-apps/api/core", () => {
  class Channel {
    onmessage: (msg: unknown) => void = () => {};
    constructor() {
      channels.push(this);
    }
  }
  return { invoke, Channel, isTauri: () => true };
});

import {
  attach,
  connectedMeters,
  disconnect,
  meterBackend,
  setRate,
  type TriggerRequest,
} from "./backend";

const A = "usb:1:7";
const B = "demo:pd-9v-2a";

beforeEach(() => {
  invoke.mockReset();
  invoke.mockResolvedValue(undefined);
  channels.length = 0;
});

describe("AC-1HHT03.3: a tile's actions name its meter", () => {
  it("names the meter in the session commands", async () => {
    const b = meterBackend(B);
    await b.disconnect();
    await b.setRate(10);
    await b.cancelWait();
    await b.resetStats();
    await b.clearHistory();
    expect(invoke).toHaveBeenNthCalledWith(1, "disconnect", { deviceId: B });
    expect(invoke).toHaveBeenNthCalledWith(2, "set_rate", { deviceId: B, rateSps: 10 });
    expect(invoke).toHaveBeenNthCalledWith(3, "cancel_wait", { deviceId: B });
    expect(invoke).toHaveBeenNthCalledWith(4, "reset_stats", { deviceId: B });
    expect(invoke).toHaveBeenNthCalledWith(5, "clear_history", { deviceId: B });
  });

  it("names the meter in recording, export, stats and history", async () => {
    const a = meterBackend(A);
    await a.startRecording("a.csv");
    await a.stopRecording();
    await a.stats();
    await a.historyGaps(0, 10.5);
    await a.historyRange(["vbus"], 0, 1000, 100);
    await a.exportSession("a-export.csv", () => {});
    expect(invoke).toHaveBeenNthCalledWith(1, "start_recording", {
      deviceId: A,
      suggestedName: "a.csv",
    });
    expect(invoke).toHaveBeenNthCalledWith(2, "stop_recording", { deviceId: A });
    expect(invoke).toHaveBeenNthCalledWith(3, "stats", { deviceId: A });
    expect(invoke).toHaveBeenNthCalledWith(4, "history_gaps", { deviceId: A, t0Ms: 0, t1Ms: 11 });
    expect(invoke).toHaveBeenNthCalledWith(5, "history_range", {
      deviceId: A,
      series: ["vbus"],
      t0Ms: 0,
      t1Ms: 1000,
      buckets: 100,
    });
    expect(invoke).toHaveBeenNthCalledWith(6, "export_session", {
      deviceId: A,
      suggestedName: "a-export.csv",
      onProgress: channels[0],
    });
  });

  it("names the meter in the PD, settings and records views", async () => {
    const b = meterBackend(B);
    await b.pdStart();
    await b.pdStop();
    await b.pdDetails(3);
    await b.pdClear();
    await b.pdSave("b.pdcap");
    await b.meterSettings();
    await b.recordsList();
    await b.recordDownload(2);
    expect(invoke.mock.calls).toEqual([
      ["pd_start", { deviceId: B }],
      ["pd_stop", { deviceId: B }],
      ["pd_details", { deviceId: B, index: 3 }],
      ["pd_clear", { deviceId: B }],
      ["pd_save", { deviceId: B, suggestedName: "b.pdcap" }],
      ["meter_settings", { deviceId: B }],
      ["records_list", { deviceId: B }],
      ["record_download", { deviceId: B, index: 2 }],
    ]);
  });

  it("names the meter in the trigger commands", async () => {
    const pd9v: TriggerRequest = {
      protocol: "pd",
      position: 2,
      voltage_mv: null,
      current_ma: null,
    };
    const a = meterBackend(A);
    await a.triggerOpen();
    await a.triggerDetect();
    await a.triggerList("pd");
    await a.triggerRisk(pd9v);
    await a.triggerRequest(pd9v, true);
    await a.triggerConsoleRisk("pd req=1");
    await a.triggerConsoleSend("pd req=1", false);
    await a.triggerBackTo5v();
    await a.triggerClose();
    expect(invoke.mock.calls).toEqual([
      ["trigger_open", { deviceId: A }],
      ["trigger_detect", { deviceId: A }],
      ["trigger_list", { deviceId: A, protocol: "pd" }],
      ["trigger_risk", { deviceId: A, request: pd9v }],
      ["trigger_request", { deviceId: A, request: pd9v, confirmed: true }],
      ["trigger_console_risk", { deviceId: A, line: "pd req=1" }],
      ["trigger_console_send", { deviceId: A, line: "pd req=1", confirmed: false }],
      ["trigger_back_to_5v", { deviceId: A }],
      ["trigger_close", { deviceId: A }],
    ]);
  });

  it("keeps two tiles apart", async () => {
    await meterBackend(A).setRate(2);
    await meterBackend(B).setRate(1000);
    expect(invoke.mock.calls).toEqual([
      ["set_rate", { deviceId: A, rateSps: 2 }],
      ["set_rate", { deviceId: B, rateSps: 1000 }],
    ]);
  });
});

describe("AC-1HHT03.1: single-meter use is unchanged", () => {
  it("still sends the plain commands without a device id", async () => {
    await disconnect();
    await setRate(50);
    expect(invoke.mock.calls).toEqual([["disconnect"], ["set_rate", { rateSps: 50 }]]);
  });
});

describe("review major 3: a reload picks up every running meter", () => {
  it("lists the running meters and attaches one by its device id", async () => {
    invoke.mockResolvedValueOnce([A, B]);
    await expect(connectedMeters()).resolves.toEqual([A, B]);
    expect(invoke).toHaveBeenNthCalledWith(1, "running_meters");
    invoke.mockResolvedValueOnce(null);
    await attach(() => {}, B);
    expect(invoke).toHaveBeenNthCalledWith(2, "attach", { deviceId: B, onEvent: channels[0] });
  });

  it("keeps the single-meter attach without a device id", async () => {
    invoke.mockResolvedValueOnce(null);
    await attach(() => {});
    expect(invoke).toHaveBeenCalledWith("attach", { onEvent: channels[0] });
  });
});
