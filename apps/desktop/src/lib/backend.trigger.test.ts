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
  meterBackend,
  pdCableSeen,
  triggerArmed,
  triggerBackTo5v,
  triggerCableRead,
  triggerChargerGone,
  triggerClose,
  triggerDetect,
  triggerDisarm,
  triggerList,
  triggerOpen,
  triggerPdEntered,
  triggerRequest,
  triggerRisk,
  type CableRead,
  type CableView,
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

describe('AC-AXHK2N.2, .5: the "don\'t ask again" commands', () => {
  const REPLY: TriggerReply = {
    command: "pd req=2",
    requested_mv: 9000,
    requested_ma: 3000,
    delivered_mv: 8865,
    delivered_ma: 0,
    reply: null,
  };

  it("sends remember only when it is true", async () => {
    invoke.mockResolvedValue(REPLY);
    await triggerRequest(PD_9V, true, true);
    expect(invoke).toHaveBeenLastCalledWith("trigger_request", {
      request: PD_9V,
      confirmed: true,
      remember: true,
    });
    await triggerRequest(PD_9V, true, false);
    expect(invoke).toHaveBeenLastCalledWith("trigger_request", { request: PD_9V, confirmed: true });
    await triggerRequest(PD_9V, false);
    expect(invoke).toHaveBeenLastCalledWith("trigger_request", {
      request: PD_9V,
      confirmed: false,
    });
  });

  it("reports the armed state", async () => {
    invoke.mockResolvedValue(true);
    await expect(triggerArmed()).resolves.toBe(true);
    expect(invoke).toHaveBeenLastCalledWith("trigger_armed");
  });

  it("clears the armed state", async () => {
    invoke.mockResolvedValue(undefined);
    await triggerDisarm();
    expect(invoke).toHaveBeenLastCalledWith("trigger_disarm");
  });
});

describe("AC-AXHK2N.9, .10: the charger gone command", () => {
  it("reports what the backend says", async () => {
    invoke.mockResolvedValue(true);
    await expect(triggerChargerGone()).resolves.toBe(true);
    expect(invoke).toHaveBeenCalledExactlyOnceWith("trigger_charger_gone");
    invoke.mockResolvedValue(false);
    await expect(triggerChargerGone()).resolves.toBe(false);
  });

  it("a meter's own command names the meter", async () => {
    invoke.mockResolvedValue(true);
    await expect(meterBackend("usb:1:7").triggerChargerGone()).resolves.toBe(true);
    expect(invoke).toHaveBeenLastCalledWith("trigger_charger_gone", { deviceId: "usb:1:7" });
  });
});

describe("AC-AXHK2P.1, .6: the Cable card's commands", () => {
  const CABLE: CableView = {
    vid: 0x05ac,
    pid: 0x8254,
    active: true,
    max_voltage_v: 50,
    max_current_ma: 5000,
    epr_capable: true,
    speed: "USB4 Gen 3",
    vendor: "Apple",
  };

  it("reads the cable with no argument: nothing from the page selects the exemption", async () => {
    const found: CableRead = { kind: "found", cable: CABLE };
    invoke.mockResolvedValue(found);
    await expect(triggerCableRead()).resolves.toEqual(found);
    expect(invoke).toHaveBeenCalledExactlyOnceWith("trigger_cable_read");
  });

  it("passes the backend's no-answer through", async () => {
    const none: CableRead = { kind: "no_answer" };
    invoke.mockResolvedValue(none);
    await expect(triggerCableRead()).resolves.toEqual(none);
  });

  it("asks for the cable a capture holds", async () => {
    invoke.mockResolvedValue(CABLE);
    await expect(pdCableSeen()).resolves.toEqual(CABLE);
    expect(invoke).toHaveBeenCalledExactlyOnceWith("pd_cable_seen");
    invoke.mockResolvedValue(null);
    await expect(pdCableSeen()).resolves.toBeNull();
  });

  it("a meter's own commands name the meter", async () => {
    invoke.mockResolvedValue({ kind: "no_answer" });
    const meter = meterBackend("usb:1:7");
    await meter.triggerCableRead();
    expect(invoke).toHaveBeenLastCalledWith("trigger_cable_read", { deviceId: "usb:1:7" });
    invoke.mockResolvedValue(null);
    await meter.pdCableSeen();
    expect(invoke).toHaveBeenLastCalledWith("pd_cable_seen", { deviceId: "usb:1:7" });
  });
});

describe("AC-AXHK2P.13: whether PD is entered comes from the backend", () => {
  it("asks the backend with no argument", async () => {
    invoke.mockResolvedValue(true);
    await expect(triggerPdEntered()).resolves.toBe(true);
    expect(invoke).toHaveBeenCalledExactlyOnceWith("trigger_pd_entered");
    invoke.mockResolvedValue(false);
    await expect(triggerPdEntered()).resolves.toBe(false);
  });

  it("a meter's own question names the meter", async () => {
    invoke.mockResolvedValue(false);
    await meterBackend("usb:1:7").triggerPdEntered();
    expect(invoke).toHaveBeenLastCalledWith("trigger_pd_entered", { deviceId: "usb:1:7" });
  });
});

describe("AC-AXHK2N.15: leaving the tab names itself", () => {
  it("a leave goes to the backend as one", async () => {
    invoke.mockResolvedValue(undefined);
    await triggerBackTo5v(true);
    expect(invoke).toHaveBeenLastCalledWith("trigger_back_to_5v", { leaving: true });
  });

  it("a press of Back to 5 V sends no leave", async () => {
    invoke.mockResolvedValue(undefined);
    await triggerBackTo5v();
    expect(invoke).toHaveBeenLastCalledWith("trigger_back_to_5v");
    await triggerBackTo5v(false);
    expect(invoke).toHaveBeenLastCalledWith("trigger_back_to_5v");
  });

  it("a meter's own leave names the meter", async () => {
    invoke.mockResolvedValue(undefined);
    await meterBackend("usb:1:7").triggerBackTo5v(true);
    expect(invoke).toHaveBeenLastCalledWith("trigger_back_to_5v", {
      leaving: true,
      deviceId: "usb:1:7",
    });
  });
});
