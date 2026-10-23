/**
 * FR DGSQ6S: the trigger commands of the Tauri backend (`src-tauri/src/trigger.rs`).
 * The request carries an explicit `confirmed` flag; the backend refuses an above-5 V
 * request without it.
 */

import { beforeEach, describe, expect, it, vi } from "vitest";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => {
  class Channel {
    onmessage: (msg: unknown) => void = () => {};
  }
  return { invoke, Channel, isTauri: () => true };
});

import {
  TRIGGER_PROTOCOLS,
  triggerBackTo5v,
  triggerClose,
  triggerDetect,
  triggerList,
  triggerOpen,
  triggerRequest,
  triggerRisk,
  type ProtocolSection,
  type TriggerReply,
  type TriggerRequest,
} from "./backend";

beforeEach(() => {
  invoke.mockReset();
});

const PD_9V: TriggerRequest = { protocol: "pd", position: 2, voltage_mv: null, current_ma: null };

describe("AC-DGSQ6S.1: detection and the sections", () => {
  it("has one section per protocol, PD first", () => {
    expect(TRIGGER_PROTOCOLS).toEqual([
      "pd",
      "qc",
      "fcp",
      "afc",
      "sfcp",
      "scp",
      "vfcp",
      "tfcp",
      "ufcs",
    ]);
  });

  it("opens, detects and lists through the backend", async () => {
    const section: ProtocolSection = {
      protocol: "qc",
      supported: false,
      offers: [],
      limits: [],
      settings: null,
    };
    invoke.mockResolvedValueOnce(undefined);
    invoke.mockResolvedValueOnce({
      bc12: "unknown",
      apple: "not-supported",
      entries: [],
      complete: false,
    });
    invoke.mockResolvedValueOnce(section);
    await triggerOpen();
    await expect(triggerDetect()).resolves.toMatchObject({ apple: "not-supported" });
    await expect(triggerList("qc")).resolves.toEqual(section);
    expect(invoke).toHaveBeenNthCalledWith(1, "trigger_open");
    expect(invoke).toHaveBeenNthCalledWith(2, "trigger_detect");
    expect(invoke).toHaveBeenNthCalledWith(3, "trigger_list", { protocol: "qc" });
  });
});

describe("AC-DGSQ6S.3: risk and the confirmed flag", () => {
  it("asks the backend for the risk class", async () => {
    invoke.mockResolvedValue("above_5v");
    await expect(triggerRisk(PD_9V)).resolves.toBe("above_5v");
    expect(invoke).toHaveBeenCalledWith("trigger_risk", { request: PD_9V });
  });

  it("passes the confirmation with the request", async () => {
    const reply: TriggerReply = {
      command: "pd req=2",
      requested_mv: 9000,
      requested_ma: 3000,
      delivered_mv: 8865,
      delivered_ma: 0,
      reply: null,
    };
    invoke.mockResolvedValue(reply);
    await expect(triggerRequest(PD_9V, true)).resolves.toEqual(reply);
    expect(invoke).toHaveBeenCalledWith("trigger_request", { request: PD_9V, confirmed: true });
    await triggerRequest(PD_9V, false);
    expect(invoke).toHaveBeenLastCalledWith("trigger_request", {
      request: PD_9V,
      confirmed: false,
    });
  });
});

describe("AC-DGSQ6S.5: Back to 5 V and close", () => {
  it("go through the backend", async () => {
    invoke.mockResolvedValue(undefined);
    await triggerBackTo5v();
    await triggerClose();
    expect(invoke).toHaveBeenNthCalledWith(1, "trigger_back_to_5v");
    expect(invoke).toHaveBeenNthCalledWith(2, "trigger_close");
  });
});
