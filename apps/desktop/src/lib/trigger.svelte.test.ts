/**
 * FR DGSQ6S (AC .1–.5): the trigger view's state. Detection and one section per
 * protocol with unsupported ones marked; every request above 5 V waits for a
 * confirmation that names the voltage, and Cancel sends nothing; the reply, the live
 * VBUS and the 5 % / 2 s deviation flag; Back to 5 V while a session is open; leaving
 * the view returns to 5 V and closes the session.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type {
  BackendEvent,
  CableRead,
  CableView,
  ConsoleLine,
  PdSendReply,
  Detection,
  ProtocolSection,
  RiskClass,
  TriggerOffer,
  TriggerProtocol,
  TriggerReply,
  TriggerRequest,
  UiError,
} from "./backend";
import { TRIGGER_PROTOCOLS } from "./backend";
import {
  DEVIATION_DELAY_MS,
  DEVIATION_LIMIT,
  PROTOCOL_TITLES,
  TriggerPanel,
  VBUS_GONE_V,
  cableVendorText,
  confirmationText,
  detectionChecks,
  deviates,
  requestVoltageMv,
  type TriggerApi,
} from "./trigger.svelte";
import { PdConsole, type ConsoleApi } from "./console.svelte";

function offer(position: number, mv: number, kind: TriggerOffer["kind"] = "fixed"): TriggerOffer {
  return {
    position,
    kind,
    min_voltage_mv: kind === "fixed" ? mv : 3300,
    max_voltage_mv: mv,
    max_current_ma: 3000,
    max_power_mw: null,
    epr: position >= 8,
  };
}

const PD_SECTION: ProtocolSection = {
  protocol: "pd",
  supported: true,
  offers: [
    offer(1, 5000),
    offer(2, 9000),
    offer(3, 15000),
    offer(4, 20000),
    offer(5, 21000, "pps"),
  ],
  limits: [],
  settings: { pd_type: "auto", emarker: "50v-240w-epr", sink: "pps-3a" },
};

const DETECTION: Detection = {
  bc12: "supported",
  apple: "not-supported",
  entries: [
    { name: "bc1.2", support: "supported", vbus_mv: 5203, ibus_ma: 0 },
    { name: "ufcs", support: "not-supported", vbus_mv: null, ibus_ma: null },
  ],
  complete: false,
};

const CABLE: CableView = {
  vid: 0x050d,
  pid: 0x8254,
  active: false,
  max_voltage_v: 50,
  max_current_ma: 5000,
  epr_capable: true,
  speed: "USB 2.0",
  vendor: null,
};

const PD_9V: TriggerRequest = { protocol: "pd", position: 2, voltage_mv: null, current_ma: null };
const PD_5V: TriggerRequest = { protocol: "pd", position: 1, voltage_mv: null, current_ma: null };

/** A scripted backend: PD and QC supported, the rest not; records every call. */
class FakeApi implements TriggerApi {
  calls: string[] = [];
  sent: { request: TriggerRequest; confirmed: boolean }[] = [];
  supported: TriggerProtocol[] = ["pd", "qc"];
  /** What `armed` answers. */
  armedNow = false;
  /** What `cableRead` answers. */
  cableAnswer: CableRead = { kind: "found", cable: CABLE };
  /** What `cableSeen` answers. */
  cableInCapture: CableView | null = null;
  /** What `chargerGone` answers. */
  goneNow = false;
  /** How many times `chargerGone` was asked. */
  goneAsked = 0;

  async open(): Promise<void> {
    this.calls.push("open");
  }
  async detect(): Promise<Detection> {
    this.calls.push("detect");
    return DETECTION;
  }
  async list(protocol: TriggerProtocol): Promise<ProtocolSection> {
    this.calls.push(`list ${protocol}`);
    if (protocol === "pd") return PD_SECTION;
    return {
      protocol,
      supported: this.supported.includes(protocol),
      offers: [],
      limits: [],
      settings: null,
    };
  }
  async risk(request: TriggerRequest): Promise<RiskClass> {
    this.calls.push("risk");
    const mv = requestMv(request);
    return mv > 5000 ? "above_5v" : "at_most_5v";
  }
  async request(request: TriggerRequest, confirmed: boolean): Promise<TriggerReply> {
    this.calls.push(`request ${confirmed}`);
    const mv = requestMv(request);
    if (mv > 5000 && !confirmed) {
      throw { kind: "usage", message: "Above 5 V needs confirmation.", hint: null };
    }
    this.sent.push({ request, confirmed });
    return {
      command: request.protocol === "pd" ? `pd req=${request.position}` : "qc 9V",
      requested_mv: mv,
      requested_ma: 3000,
      delivered_mv: Math.round(mv * 0.985),
      delivered_ma: 0,
      reply: null,
    };
  }
  async backTo5v(leaving?: boolean): Promise<void> {
    // Leaving the view and the Back button read alike here; the flag is checked on its own.
    void leaving;
    this.calls.push("back");
  }
  async close(): Promise<void> {
    this.calls.push("close");
  }
  async armed(): Promise<boolean> {
    return this.armedNow;
  }
  async disarm(): Promise<void> {
    this.calls.push("disarm");
    this.armedNow = false;
  }
  async chargerGone(): Promise<boolean> {
    this.goneAsked += 1;
    return this.goneNow;
  }
  async cableRead(): Promise<CableRead> {
    this.calls.push("cable read");
    return this.cableAnswer;
  }
  async cableSeen(): Promise<CableView | null> {
    this.calls.push("cable seen");
    return this.cableInCapture;
  }
}

function requestMv(r: TriggerRequest): number {
  if (r.protocol === "pd") {
    if (r.voltage_mv !== null) return r.voltage_mv;
    return PD_SECTION.offers.find((o) => o.position === r.position)?.max_voltage_mv ?? 0;
  }
  if ("volts" in r) return r.volts * 1000;
  return r.voltage_mv;
}

function batch(vbus: number, tMs = 0): BackendEvent {
  return {
    type: "batch",
    data: {
      samples: [{ t_ms: tMs, vbus_v: vbus, ibus_a: 0, cc1_v: 0, cc2_v: 0, dp_v: 0, dm_v: 0 }],
      temp_c: null,
      stats: {
        elapsed_ms: 0,
        samples: 1,
        charge_ah: 0,
        energy_wh: 0,
        vbus: null,
        ibus: null,
        power: null,
      },
    },
  };
}

async function detected(): Promise<{ api: FakeApi; panel: TriggerPanel }> {
  const api = new FakeApi();
  const panel = new TriggerPanel(api);
  await panel.detect();
  return { api, panel };
}

/** Requests 9 V and confirms it. */
async function at9V(panel: TriggerPanel): Promise<void> {
  await panel.ask(PD_9V);
  await panel.confirm();
}

describe("AC-DGSQ6S.1: detection and one section per protocol", () => {
  it("has a section per protocol, PD first, before anything is detected", () => {
    const panel = new TriggerPanel(new FakeApi());
    expect(panel.sections.map((s) => s.protocol)).toEqual([...TRIGGER_PROTOCOLS]);
    expect(panel.sections[0]?.title).toBe(PROTOCOL_TITLES.pd);
    expect(panel.sections.every((s) => s.state === "pending")).toBe(true);
    expect(panel.sessionOpen).toBe(false);
  });

  it("detect opens the session, keeps the result and lists every protocol", async () => {
    const { api, panel } = await detected();
    expect(api.calls[0]).toBe("open");
    expect(api.calls).toContain("detect");
    for (const p of TRIGGER_PROTOCOLS) expect(api.calls).toContain(`list ${p}`);
    expect(panel.sessionOpen).toBe(true);
    expect(panel.detection).toEqual(DETECTION);
  });

  it("marks the protocols the charger does not support", async () => {
    const { panel } = await detected();
    const state = Object.fromEntries(panel.sections.map((s) => [s.protocol, s.state]));
    expect(state).toEqual({
      pd: "supported",
      qc: "supported",
      fcp: "not-supported",
      afc: "not-supported",
      sfcp: "not-supported",
      scp: "not-supported",
      vfcp: "not-supported",
      tfcp: "not-supported",
      ufcs: "not-supported",
    });
  });

  it("a second detect does not open a second session", async () => {
    const { api, panel } = await detected();
    await panel.detect();
    expect(api.calls.filter((c) => c === "open")).toHaveLength(1);
  });

  it("a failing backend call becomes the panel's error, not a throw", async () => {
    const api = new FakeApi();
    api.detect = async () => {
      throw { kind: "timeout", message: "The meter did not answer.", hint: null };
    };
    const panel = new TriggerPanel(api);
    await expect(panel.detect()).resolves.toBeUndefined();
    expect(panel.error).toMatchObject({ kind: "timeout" });
  });
});

describe("AC-DGSQ6S.2: the PD section", () => {
  it("keeps the decoded offers and the emulation settings", async () => {
    const { panel } = await detected();
    const pd = panel.sections.find((s) => s.protocol === "pd");
    expect(pd?.section?.offers.map((o) => o.position)).toEqual([1, 2, 3, 4, 5]);
    expect(pd?.section?.settings).toEqual(PD_SECTION.settings);
  });

  it("knows the voltage of a fixed offer and of a PPS request", () => {
    expect(requestVoltageMv(PD_9V, [PD_SECTION])).toBe(9000);
    expect(
      requestVoltageMv({ protocol: "pd", position: 5, voltage_mv: 7200, current_ma: 1000 }, [
        PD_SECTION,
      ]),
    ).toBe(7200);
    expect(requestVoltageMv({ protocol: "qc", volts: 12 }, [])).toBe(12000);
    expect(requestVoltageMv({ protocol: "qc3", voltage_mv: 6600 }, [])).toBe(6600);
    expect(
      requestVoltageMv({ protocol: "pd", position: 7, voltage_mv: null, current_ma: null }, [
        PD_SECTION,
      ]),
    ).toBeNull();
  });
});

describe("AC-DGSQ6S.3: confirmation above 5 V", () => {
  it("a request above 5 V waits for confirmation and sends nothing yet", async () => {
    const { api, panel } = await detected();
    await panel.ask(PD_9V);
    expect(panel.pending).not.toBeNull();
    expect(panel.pending?.voltageMv).toBe(9000);
    expect(api.sent).toEqual([]);
  });

  it("the dialog names the voltage and asks about the output", async () => {
    const { panel } = await detected();
    await panel.ask(PD_9V);
    expect(panel.pending?.text).toMatch(/\b9(\.0+)? V\b/);
    expect(panel.pending?.text).toMatch(/output/i);
    expect(confirmationText(20000)).toMatch(/\b20(\.0+)? V\b/);
    expect(confirmationText(20000)).toMatch(/output/i);
  });

  it("Cancel sends nothing", async () => {
    const { api, panel } = await detected();
    const before = api.calls.length;
    await panel.ask(PD_9V);
    panel.cancel();
    expect(panel.pending).toBeNull();
    expect(api.sent).toEqual([]);
    expect(api.calls.slice(before).filter((c) => c.startsWith("request"))).toEqual([]);
    expect(panel.reply).toBeNull();
  });

  it("Confirm sends the request with the confirmation", async () => {
    const { api, panel } = await detected();
    await at9V(panel);
    expect(panel.pending).toBeNull();
    expect(api.sent).toEqual([{ request: PD_9V, confirmed: true }]);
  });

  it("a request at 5 V is sent at once, unconfirmed", async () => {
    const { api, panel } = await detected();
    await panel.ask(PD_5V);
    expect(panel.pending).toBeNull();
    expect(api.sent).toEqual([{ request: PD_5V, confirmed: false }]);
  });

  it("the risk class comes from the backend, not the UI's own guess", async () => {
    const { api, panel } = await detected();
    // The backend says this one is above 5 V (e.g. QC 3.0 steps up).
    api.risk = async () => "above_5v";
    await panel.ask({ protocol: "pd", position: 1, voltage_mv: null, current_ma: null });
    expect(panel.pending).not.toBeNull();
    expect(api.sent).toEqual([]);
  });
});

describe("AC-DGSQ6S.4: reply, live VBUS and the 5 % / 2 s deviation flag", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("deviates() is strict at 5 %", () => {
    expect(DEVIATION_LIMIT).toBe(0.05);
    expect(DEVIATION_DELAY_MS).toBe(2000);
    expect(deviates(9.0, 9000)).toBe(false);
    expect(deviates(8.56, 9000)).toBe(false);
    expect(deviates(8.5, 9000)).toBe(true);
    expect(deviates(9.5, 9000)).toBe(true);
    expect(deviates(9.44, 9000)).toBe(false);
  });

  it("shows the reply and the live VBUS", async () => {
    const { panel } = await detected();
    await at9V(panel);
    expect(panel.reply?.requested_mv).toBe(9000);
    expect(panel.requestedMv).toBe(9000);
    panel.handleEvent(batch(8.87));
    expect(panel.vbusV).toBeCloseTo(8.87);
  });

  it("flags VBUS more than 5 % off only once 2 s have passed", async () => {
    const { panel } = await detected();
    await at9V(panel);
    panel.handleEvent(batch(8.0));
    vi.advanceTimersByTime(DEVIATION_DELAY_MS - 1);
    expect(panel.deviation).toBe(false);
    vi.advanceTimersByTime(1);
    panel.handleEvent(batch(8.0));
    expect(panel.deviation).toBe(true);
  });

  it("does not flag VBUS within 5 %", async () => {
    const { panel } = await detected();
    await at9V(panel);
    panel.handleEvent(batch(8.7));
    vi.advanceTimersByTime(DEVIATION_DELAY_MS + 500);
    panel.handleEvent(batch(8.7));
    expect(panel.deviation).toBe(false);
  });

  it("keeps following the live VBUS after the 2 s", async () => {
    const { panel } = await detected();
    await at9V(panel);
    panel.handleEvent(batch(8.9));
    vi.advanceTimersByTime(DEVIATION_DELAY_MS + 100);
    panel.handleEvent(batch(8.9));
    expect(panel.deviation).toBe(false);
    panel.handleEvent(batch(7.0));
    expect(panel.deviation).toBe(true);
    panel.handleEvent(batch(8.95));
    expect(panel.deviation).toBe(false);
  });

  it("a new request clears the flag and restarts the 2 s", async () => {
    const { panel } = await detected();
    await at9V(panel);
    panel.handleEvent(batch(7.0));
    vi.advanceTimersByTime(DEVIATION_DELAY_MS + 100);
    panel.handleEvent(batch(7.0));
    expect(panel.deviation).toBe(true);
    await panel.ask({ protocol: "pd", position: 3, voltage_mv: null, current_ma: null });
    await panel.confirm();
    expect(panel.deviation).toBe(false);
    panel.handleEvent(batch(7.0));
    expect(panel.deviation).toBe(false);
  });
});

describe("AC-DGSQ6S.5: Back to 5 V and leaving the view", () => {
  it("Back to 5 V is shown only while a session is open", async () => {
    const api = new FakeApi();
    const panel = new TriggerPanel(api);
    expect(panel.showBackTo5v).toBe(false);
    await panel.detect();
    expect(panel.showBackTo5v).toBe(true);
    await at9V(panel);
    expect(panel.showBackTo5v).toBe(true);
  });

  it("Back to 5 V goes through the backend and keeps the session", async () => {
    const { api, panel } = await detected();
    await at9V(panel);
    await panel.backTo5v();
    expect(api.calls.at(-1)).toBe("back");
    expect(panel.sessionOpen).toBe(true);
    expect(panel.deviation).toBe(false);
  });

  it("leaving the view returns to 5 V, then closes the session", async () => {
    const { api, panel } = await detected();
    await at9V(panel);
    await panel.leave();
    const tail = api.calls.slice(-2);
    expect(tail).toEqual(["back", "close"]);
    expect(panel.sessionOpen).toBe(false);
    expect(panel.showBackTo5v).toBe(false);
  });

  it("leaving while the session is still opening closes it once it opens", async () => {
    const api = new FakeApi();
    let release: () => void = () => {};
    api.open = () =>
      new Promise<void>((resolve) => {
        api.calls.push("open");
        release = resolve;
      });
    const panel = new TriggerPanel(api);
    const detecting = panel.detect();
    await panel.leave();
    release();
    await detecting;
    expect(api.calls.at(-1)).toBe("close");
    expect(api.calls).not.toContain("detect");
    expect(panel.sessionOpen).toBe(false);
  });

  it("leaving mid-detect stops listing the other protocols", async () => {
    const api = new FakeApi();
    const panel = new TriggerPanel(api);
    const list = api.list.bind(api);
    api.list = async (protocol) => {
      const section = await list(protocol);
      if (protocol === "pd") await panel.leave();
      return section;
    };
    await panel.detect();
    expect(api.calls.filter((c) => c.startsWith("list"))).toEqual(["list pd"]);
    expect(panel.error).toBeNull();
  });

  it("a risk answer that lands after leaving opens no dialog", async () => {
    const { api, panel } = await detected();
    const risk = api.risk.bind(api);
    api.risk = async (r) => {
      const answer = await risk(r);
      await panel.leave();
      return answer;
    };
    await panel.ask(PD_9V);
    expect(panel.pending).toBeNull();
  });

  it("a reply that lands after leaving leaves no result behind", async () => {
    const { api, panel } = await detected();
    const request = api.request.bind(api);
    api.request = async (r, confirmed) => {
      const reply = await request(r, confirmed);
      await panel.leave();
      return reply;
    };
    await panel.ask(PD_5V);
    expect(panel.reply).toBeNull();
  });

  it("a list error after leaving is not shown", async () => {
    const api = new FakeApi();
    const panel = new TriggerPanel(api);
    api.list = async (protocol) => {
      if (protocol === "pd") {
        await panel.leave();
        throw { kind: "usage", message: "The trigger session is closed.", hint: null };
      }
      return { protocol, supported: false, offers: [], limits: [], settings: null };
    };
    await panel.detect();
    expect(panel.error).toBeNull();
  });

  it("leaving still closes when the return to 5 V fails", async () => {
    const { api, panel } = await detected();
    api.backTo5v = async () => {
      api.calls.push("back");
      throw { kind: "timeout", message: "No answer.", hint: null };
    };
    await panel.leave();
    expect(api.calls.slice(-2)).toEqual(["back", "close"]);
    expect(panel.sessionOpen).toBe(false);
  });

  it("leaving without a session sends nothing", async () => {
    const api = new FakeApi();
    const panel = new TriggerPanel(api);
    await panel.leave();
    expect(api.calls).toEqual([]);
  });

  it("leaving drops a pending confirmation without sending it", async () => {
    const { api, panel } = await detected();
    await panel.ask(PD_9V);
    await panel.leave();
    expect(panel.pending).toBeNull();
    expect(api.sent).toEqual([]);
  });

  it("a disconnect ends the session in the view (the backend closed it)", async () => {
    const { panel } = await detected();
    await panel.ask(PD_9V);
    panel.handleEvent({ type: "status", data: { state: "disconnected", error: null } });
    expect(panel.sessionOpen).toBe(false);
    expect(panel.showBackTo5v).toBe(false);
    expect(panel.pending).toBeNull();
    expect(panel.vbusV).toBeNull();
  });
});

describe("AC-DGSQ6S.9: a failed request shows its failure and reason", () => {
  const FAILURES: { name: string; error: UiError }[] = [
    {
      name: "an error reply",
      error: {
        kind: "protocol",
        message: "The charger refused the request (req error 252).",
        hint: null,
      },
    },
    {
      name: "a timeout",
      error: { kind: "timeout", message: "The meter did not answer in time.", hint: null },
    },
    {
      name: "a refusal",
      error: { kind: "usage", message: "The charger offers nothing at position 2.", hint: null },
    },
  ];

  for (const { name, error } of FAILURES) {
    it(`a confirmed request that fails with ${name} shows it in the request result`, async () => {
      const { api, panel } = await detected();
      api.request = async () => {
        throw error;
      };
      await at9V(panel);
      expect(panel.requestFailure).toEqual(error);
      // The result is not empty: it says the request failed and why.
      expect(panel.requestFailure?.message).toContain(error.message);
    });
  }

  it("a failure replaces the previous request's reply", async () => {
    const { api, panel } = await detected();
    await at9V(panel);
    expect(panel.reply).not.toBeNull();
    api.request = async () => {
      throw { kind: "protocol", message: "req error 252", hint: null };
    };
    await at9V(panel);
    expect(panel.reply).toBeNull();
    expect(panel.requestFailure?.message).toContain("252");
  });

  it("a later successful request clears the failure", async () => {
    const { api, panel } = await detected();
    const real = api.request.bind(api);
    api.request = async () => {
      throw { kind: "timeout", message: "No answer.", hint: null };
    };
    await at9V(panel);
    expect(panel.requestFailure).not.toBeNull();
    api.request = real;
    await at9V(panel);
    expect(panel.requestFailure).toBeNull();
    expect(panel.reply?.requested_mv).toBe(9000);
  });

  it("a failure at or below 5 V (no dialog) is shown too", async () => {
    const { api, panel } = await detected();
    api.request = async () => {
      throw { kind: "protocol", message: "req error 252", hint: null };
    };
    await panel.ask(PD_5V);
    expect(panel.requestFailure?.message).toContain("252");
  });
});

describe("AC-DGSQ6S.10: an out-of-sync meter stops the session's requests", () => {
  const OUT_OF_SYNC: UiError = {
    kind: "out_of_sync",
    message: "The meter's replies are out of step with the commands.",
    hint: "Power-cycle the meter (unplug it and plug it back in), then open the session again.",
  };

  it("an out-of-sync request shows the error and disables requests", async () => {
    const { api, panel } = await detected();
    expect(panel.requestsDisabled).toBe(false);
    api.request = async () => {
      throw OUT_OF_SYNC;
    };
    await at9V(panel);
    expect(panel.error).toEqual(OUT_OF_SYNC);
    expect(panel.requestsDisabled).toBe(true);
  });

  it("an out-of-sync error while listing disables requests too", async () => {
    const api = new FakeApi();
    api.list = async (protocol) => {
      api.calls.push(`list ${protocol}`);
      throw OUT_OF_SYNC;
    };
    const panel = new TriggerPanel(api);
    await panel.detect();
    expect(panel.error).toEqual(OUT_OF_SYNC);
    expect(panel.requestsDisabled).toBe(true);
  });

  it("while disabled, asking sends nothing to the backend", async () => {
    const { api, panel } = await detected();
    api.request = async () => {
      throw OUT_OF_SYNC;
    };
    await at9V(panel);
    const calls = api.calls.length;
    await panel.ask(PD_5V);
    await panel.ask(PD_9V);
    expect(panel.pending).toBeNull();
    expect(api.calls.slice(calls)).toEqual([]);
    // The power-cycle error stays on screen.
    expect(panel.error).toEqual(OUT_OF_SYNC);
  });

  it("Back to 5 V still reaches the backend and shows its VBUS warning", async () => {
    // The backend sends `pd hrst` blind, closes the session and answers with
    // an out-of-sync error saying VBUS is not confirmed.
    const { api, panel } = await detected();
    api.request = async () => {
      throw OUT_OF_SYNC;
    };
    await at9V(panel);
    const warning: UiError = {
      ...OUT_OF_SYNC,
      hint: "VBUS may still be above 5 V: the app sent a Hard Reset (or reset) and closed the session.",
    };
    api.backTo5v = async () => {
      api.calls.push("back");
      throw warning;
    };
    const calls = api.calls.length;
    await panel.backTo5v();
    expect(api.calls.slice(calls)).toEqual(["back"]);
    expect(panel.error).toEqual(warning);
    expect(panel.requestsDisabled).toBe(true);
  });

  it("other errors do not disable requests", async () => {
    const { api, panel } = await detected();
    api.request = async () => {
      throw { kind: "timeout", message: "No answer.", hint: null };
    };
    await at9V(panel);
    expect(panel.requestsDisabled).toBe(false);
  });

  it("reopening the session enables requests again", async () => {
    const { api, panel } = await detected();
    const real = api.request.bind(api);
    api.request = async () => {
      throw OUT_OF_SYNC;
    };
    await at9V(panel);
    expect(panel.requestsDisabled).toBe(true);
    await panel.leave();
    api.request = real;
    await panel.detect();
    expect(panel.requestsDisabled).toBe(false);
    await at9V(panel);
    expect(panel.reply?.requested_mv).toBe(9000);
  });
});

describe("AC-DGSQ6R.7: a detection whose sync check fails stops the session", () => {
  // The backend's detect fails with `out_of_sync` when the meter answers the
  // `pdm parm` check after a cut list with another command's reply.
  const OUT_OF_SYNC: UiError = {
    kind: "out_of_sync",
    message: "The meter's replies are out of step with the commands.",
    hint: "Power-cycle the meter (unplug it and plug it back in), then open the session again.",
  };

  it("shows the power-cycle error, lists nothing and disables requests", async () => {
    const api = new FakeApi();
    api.detect = async () => {
      api.calls.push("detect");
      throw OUT_OF_SYNC;
    };
    const panel = new TriggerPanel(api);
    await panel.detect();
    expect(panel.error).toEqual(OUT_OF_SYNC);
    expect(panel.requestsDisabled).toBe(true);
    expect(api.calls.filter((c) => c.startsWith("list"))).toEqual([]);
    const calls = api.calls.length;
    await panel.ask(PD_5V);
    expect(api.calls.slice(calls)).toEqual([]);
  });
});

describe("AC-DGSQ6R.8: the entry checks and the list disagree", () => {
  // Rule: `detectionChecks(d)` gives the BC1.2 and Apple rows of the Detection card.
  // When a check (`entry bc` / `entry apple`) and the automatic list's matching line
  // (`bc1.2` / an `Apple …` mode line) say supported vs not supported, the row names
  // both sources: "entry bc: not supported · list: supported". Otherwise the row is the
  // check's plain result ("Supported", "Not supported", "Unknown").
  function d(
    bc12: Detection["bc12"],
    apple: Detection["apple"],
    entries: [string, Detection["bc12"]][],
  ): Detection {
    return {
      bc12,
      apple,
      entries: entries.map(([name, support]) => ({ name, support, vbus_mv: null, ibus_ma: null })),
      complete: false,
    };
  }

  function row(det: Detection, label: string): string | undefined {
    return detectionChecks(det).find((r) => r.label === label)?.text;
  }

  it("names both sources for BC1.2 (device trace 2026-10-23)", () => {
    const det = d("not-supported", "not-supported", [
      ["bc1.2", "supported"],
      ["ufcs", "not-supported"],
    ]);
    expect(detectionChecks(det).map((r) => r.label)).toEqual(["BC1.2", "Apple"]);
    expect(row(det, "BC1.2")).toBe("entry bc: not supported · list: supported");
    expect(row(det, "Apple")).toBe("Not supported");
  });

  it("names both sources for Apple against an Apple mode line", () => {
    const det = d("supported", "not-supported", [
      ["bc1.2", "supported"],
      ["Apple 2.4A", "supported"],
    ]);
    expect(row(det, "Apple")).toBe("entry apple: not supported · list: supported (Apple 2.4A)");
    expect(row(det, "BC1.2")).toBe("Supported");
  });

  it("names every clashing Apple mode line, like the library's disagreements", () => {
    const det = d("unknown", "not-supported", [
      ["Apple 2.4A", "supported"],
      ["Apple 1A", "unknown"],
      ["Apple 2.1A", "supported"],
    ]);
    expect(row(det, "Apple")).toBe(
      "entry apple: not supported · list: supported (Apple 2.4A, Apple 2.1A)",
    );
  });

  it("names both sources the other way round", () => {
    const det = d("supported", "unknown", [["bc1.2", "not-supported"]]);
    expect(row(det, "BC1.2")).toBe("entry bc: supported · list: not supported");
  });

  it("keeps the plain result when they agree or the list has no matching line", () => {
    expect(row(d("not-supported", "unknown", [["bc1.2", "not-supported"]]), "BC1.2")).toBe(
      "Not supported",
    );
    expect(row(d("not-supported", "unknown", [["ufcs", "not-supported"]]), "BC1.2")).toBe(
      "Not supported",
    );
    expect(row(d("unknown", "unknown", []), "Apple")).toBe("Unknown");
  });

  it("never shows an unqualified not supported next to a supported list line", () => {
    const det = d("not-supported", "not-supported", [
      ["bc1.2", "supported"],
      ["Apple 2.4A", "supported"],
    ]);
    for (const r of detectionChecks(det)) {
      expect(r.text).not.toBe("Not supported");
      expect(r.text).toMatch(/^entry (bc|apple): not supported · list: supported( \(.+\))?$/);
    }
  });
});

describe("AC-4PJRVH.1: the result never mixes an old and a new request", () => {
  it("Back to 5 V drops the old request's command and delivered values", async () => {
    const { panel } = await detected();
    await at9V(panel);
    expect(panel.reply?.command).toBe("pd req=2");
    expect(panel.reply?.delivered_mv).toBe(8865);

    await panel.backTo5v();

    // The card reads `requestedMv` next to `reply`: 5 V beside `pd req=2` is the mix.
    expect(panel.requestedMv).toBe(5000);
    expect(panel.reply?.command).not.toBe("pd req=2");
    expect(panel.reply?.delivered_mv).not.toBe(8865);
  });

  it("Back to 5 V drops an old request's failure", async () => {
    const { api, panel } = await detected();
    const refused: UiError = { kind: "protocol", message: "req error 252", hint: null };
    api.request = async () => {
      throw refused;
    };
    await panel.ask(PD_5V);
    expect(panel.requestFailure).toEqual(refused);

    await panel.backTo5v();

    expect(panel.requestFailure).toBeNull();
  });

  it("a Back to 5 V that failed leaves the last request's result as it was", async () => {
    const { api, panel } = await detected();
    await at9V(panel);
    api.backTo5v = async () => {
      throw { kind: "protocol", message: "no answer", hint: null } satisfies UiError;
    };

    await panel.backTo5v();

    // VBUS is still at the old request, and the card still says so.
    expect(panel.reply?.command).toBe("pd req=2");
    expect(panel.requestedMv).toBe(9000);
  });

  it("a Back to 5 V answered after the session ended leaves no card behind", async () => {
    const { api, panel } = await detected();
    await at9V(panel);
    let answer = () => {};
    api.backTo5v = () => new Promise<void>((resolve) => (answer = resolve));

    const back = panel.backTo5v();
    panel.handleEvent({ type: "status", data: { state: "disconnected" } } as BackendEvent);
    answer();
    await back;

    expect(panel.reply).toBeNull();
    expect(panel.requestedMv).toBeNull();
  });
});

/**
 * FR AXHK2N: "don't ask again". The backend's rules, as the panel sees them: PD up to
 * 20 V is rememberable (position 5 here goes to 21 V), every other request above 5 V
 * is not; once armed a rememberable request is `armed` and sends unconfirmed; anything
 * at or below 5 V, Back to 5 V, Detect and closing disarm. Records the exact arguments
 * of every request.
 */
class ArmedApi extends FakeApi {
  requestArgs: unknown[][] = [];

  static rememberable(r: TriggerRequest): boolean {
    return r.protocol === "pd" && requestMv(r) <= 20000;
  }

  override async detect(): Promise<Detection> {
    this.armedNow = false;
    return super.detect();
  }
  override async risk(request: TriggerRequest): Promise<RiskClass> {
    this.calls.push("risk");
    if (requestMv(request) <= 5000) return "at_most_5v";
    if (!ArmedApi.rememberable(request)) return "above_5v";
    return this.armedNow ? "armed" : "above_5v_rememberable";
  }
  override async request(
    ...args: [request: TriggerRequest, confirmed: boolean, remember?: boolean]
  ): Promise<TriggerReply> {
    this.requestArgs.push(args);
    const [request, confirmed, remember] = args;
    const mv = requestMv(request);
    if (mv > 5000 && !confirmed && !(this.armedNow && ArmedApi.rememberable(request))) {
      throw { kind: "usage", message: "Above 5 V needs confirmation.", hint: null };
    }
    if (remember && !ArmedApi.rememberable(request)) {
      throw { kind: "usage", message: "This request cannot be remembered.", hint: null };
    }
    if (mv <= 5000) this.armedNow = false;
    if (remember) this.armedNow = true;
    return super.request(request, true);
  }
  override async backTo5v(): Promise<void> {
    this.armedNow = false;
    return super.backTo5v();
  }
  override async close(): Promise<void> {
    this.armedNow = false;
    return super.close();
  }
}

const PD_15V: TriggerRequest = { protocol: "pd", position: 3, voltage_mv: null, current_ma: null };
const PPS_12V: TriggerRequest = {
  protocol: "pd",
  position: 5,
  voltage_mv: 12000,
  current_ma: 1000,
};
const PPS_21V: TriggerRequest = {
  protocol: "pd",
  position: 5,
  voltage_mv: 21000,
  current_ma: 1000,
};
const QC_9V: TriggerRequest = { protocol: "qc", volts: 9 };

async function rememberPanel(): Promise<{ api: ArmedApi; panel: TriggerPanel }> {
  const api = new ArmedApi();
  const panel = new TriggerPanel(api);
  await panel.detect();
  return { api, panel };
}

/** A panel armed by a confirmed 9 V with the box checked. */
async function armedPanel(): Promise<{ api: ArmedApi; panel: TriggerPanel }> {
  const { api, panel } = await rememberPanel();
  await panel.ask(PD_9V);
  panel.remember = true;
  await panel.confirm();
  expect(panel.armed).toBe(true);
  return { api, panel };
}

describe("AC-AXHK2N.1: the checkbox", () => {
  it("is offered for a rememberable request", async () => {
    const { panel } = await rememberPanel();
    await panel.ask(PD_9V);
    expect(panel.pending?.request).toEqual(PD_9V);
    expect(panel.pending?.rememberable).toBe(true);
  });

  it("is not offered for any other request above 5 V", async () => {
    const { panel } = await rememberPanel();
    for (const request of [PPS_21V, QC_9V]) {
      await panel.ask(request);
      expect(panel.pending?.request).toEqual(request);
      expect(panel.pending?.rememberable).toBe(false);
      panel.cancel();
    }
  });

  it("starts unchecked every time a dialog opens", async () => {
    const { panel } = await rememberPanel();
    expect(panel.remember).toBe(false);
    await panel.ask(PD_9V);
    expect(panel.remember).toBe(false);
    panel.remember = true;
    panel.cancel();
    await panel.ask(PD_9V);
    expect(panel.remember).toBe(false);
    panel.remember = true;
    await panel.confirm();
    panel.cancel();
    await panel.backTo5v();
    await panel.ask(PD_15V);
    expect(panel.remember).toBe(false);
  });
});

describe("AC-AXHK2N.2: remembered requests", () => {
  it("passes remember only when the box is checked", async () => {
    const { api, panel } = await rememberPanel();
    await panel.ask(PD_9V);
    await panel.confirm();
    expect(api.requestArgs.at(-1)).toEqual([PD_9V, true]);
    expect(api.requestArgs.at(-1)).toHaveLength(2);
    expect(panel.armed).toBe(false);
    await panel.ask(PD_15V);
    panel.remember = true;
    await panel.confirm();
    expect(api.requestArgs.at(-1)).toEqual([PD_15V, true, true]);
    expect(panel.armed).toBe(true);
  });

  it("sends later rememberable requests with no dialog, unconfirmed", async () => {
    const { api, panel } = await armedPanel();
    for (const request of [PD_15V, PPS_12V]) {
      await panel.ask(request);
      expect(panel.pending).toBeNull();
      expect(api.requestArgs.at(-1)).toEqual([request, false]);
    }
    expect(panel.reply?.command).toBe("pd req=5");
    expect(panel.armed).toBe(true);
  });

  it("a checked box on a dialog that does not offer it remembers nothing", async () => {
    const { api, panel } = await rememberPanel();
    await panel.ask(PPS_21V);
    panel.remember = true;
    await panel.confirm();
    expect(api.requestArgs.at(-1)).toEqual([PPS_21V, true]);
    expect(api.requestArgs.at(-1)).toHaveLength(2);
    expect(panel.armed).toBe(false);
  });
});

describe("AC-AXHK2N.3: what always asks", () => {
  it("armed, a request that does not qualify opens the dialog and Cancel sends nothing", async () => {
    const { api, panel } = await armedPanel();
    const sent = api.requestArgs.length;
    for (const request of [PPS_21V, QC_9V]) {
      await panel.ask(request);
      expect(panel.pending?.rememberable).toBe(false);
      panel.cancel();
    }
    expect(api.requestArgs).toHaveLength(sent);
  });
});

describe("AC-AXHK2N.4: the status follows the backend", () => {
  it("Back to 5 V clears it", async () => {
    const { panel } = await armedPanel();
    await panel.backTo5v();
    expect(panel.armed).toBe(false);
  });

  it("a Back to 5 V that fails clears it too", async () => {
    const { api, panel } = await armedPanel();
    api.backTo5v = async () => {
      api.armedNow = false;
      throw { kind: "out_of_sync", message: "Out of step.", hint: null };
    };
    await panel.backTo5v();
    expect(panel.armed).toBe(false);
  });

  it("leaving the view clears it", async () => {
    const { panel } = await armedPanel();
    await panel.leave();
    expect(panel.armed).toBe(false);
  });

  it("a meter that disconnects clears it", async () => {
    const { panel } = await armedPanel();
    panel.handleEvent({
      type: "status",
      data: { state: "disconnected" },
    } as BackendEvent);
    expect(panel.armed).toBe(false);
  });

  it("a new page starts unarmed and shows what an armed backend reports", async () => {
    const api = new ArmedApi();
    api.armedNow = true;
    const panel = new TriggerPanel(api);
    expect(panel.armed).toBe(false);
    await panel.refreshArmed();
    expect(panel.armed).toBe(true);
  });

  it("a page reload does not arm a backend that is not armed", async () => {
    const api = new ArmedApi();
    const panel = new TriggerPanel(api);
    await panel.refreshArmed();
    expect(panel.armed).toBe(false);
    expect(api.armedNow).toBe(false);
  });
});

describe("AC-AXHK2N.5: Clear", () => {
  it("disarms the backend at once, and the dialog comes back", async () => {
    const { api, panel } = await armedPanel();
    await panel.clearArmed();
    expect(api.calls).toContain("disarm");
    expect(api.armedNow).toBe(false);
    expect(panel.armed).toBe(false);
    await panel.ask(PD_15V);
    expect(panel.pending?.rememberable).toBe(true);
  });
});

describe("AC-AXHK2N.6: the backend decides", () => {
  it("a refused arming request leaves the panel unarmed, with the failure shown", async () => {
    const { api, panel } = await rememberPanel();
    await panel.ask(PD_9V);
    api.request = async () => {
      throw { kind: "usage", message: "This request cannot be remembered.", hint: null };
    };
    panel.remember = true;
    await panel.confirm();
    expect(panel.armed).toBe(false);
    expect(panel.error).toMatchObject({ kind: "usage" });
  });
});

describe("AC-AXHK2N.8: the status follows every other way it ends", () => {
  it("a request at 5 V clears it", async () => {
    const { panel } = await armedPanel();
    await panel.ask(PD_5V);
    expect(panel.armed).toBe(false);
  });

  it("a request that fails clears it", async () => {
    const { api, panel } = await armedPanel();
    api.request = async () => {
      api.armedNow = false;
      throw { kind: "protocol", message: "The charger refused the request.", hint: null };
    };
    await panel.ask(PD_15V);
    expect(panel.armed).toBe(false);
  });

  it("a held request whose offer changed goes back to asking", async () => {
    const { api, panel } = await armedPanel();
    const send = api.request;
    api.request = async () => {
      api.armedNow = false;
      throw { kind: "usage", message: "The charger's offers changed.", hint: null };
    };
    await panel.ask(PD_15V);
    expect(panel.error).toMatchObject({ kind: "usage" });
    expect(panel.armed).toBe(false);
    api.request = send;
    await panel.ask(PD_15V);
    expect(panel.pending?.rememberable).toBe(true);
  });

  it("a risk that is no longer armed takes the status down with the dialog", async () => {
    const { api, panel } = await armedPanel();
    api.armedNow = false;
    api.risk = async () => "above_5v_rememberable";
    await panel.ask(PD_15V);
    expect(panel.pending).not.toBeNull();
    expect(panel.armed).toBe(false);
  });

  it("a confirmed request the backend disarmed on takes the status down", async () => {
    const { api, panel } = await armedPanel();
    api.risk = async () => "above_5v";
    await panel.ask(PD_15V);
    api.armedNow = false;
    await panel.confirm();
    expect(panel.armed).toBe(false);
  });

  it("Detect clears it", async () => {
    const { panel } = await armedPanel();
    await panel.detect();
    expect(panel.armed).toBe(false);
  });

  it("refreshArmed takes in what a console line or built message did", async () => {
    const { api, panel } = await armedPanel();
    api.armedNow = false;
    await panel.refreshArmed();
    expect(panel.armed).toBe(false);
  });

  it("a refresh that fails reads as not armed and is no error", async () => {
    const { api, panel } = await armedPanel();
    api.armed = async () => {
      throw { kind: "usage", message: "The trigger session is closed.", hint: null };
    };
    await panel.refreshArmed();
    expect(panel.armed).toBe(false);
    expect(panel.error).toBeNull();
  });
});

describe("FR AXHK2P: the Cable card", () => {
  /** An open session; the calls of the detection are forgotten. */
  async function inSession(): Promise<{ api: FakeApi; panel: TriggerPanel }> {
    const { api, panel } = await detected();
    api.calls = [];
    return { api, panel };
  }

  /** A `cableRead` that answers only when `release` is called. */
  function heldRead(api: FakeApi): (answer: CableRead) => void {
    let release: (answer: CableRead) => void = () => {};
    api.cableRead = () => {
      api.calls.push("cable read");
      return new Promise((resolve) => (release = resolve));
    };
    return (answer) => release(answer);
  }

  describe("AC-AXHK2P.1: one read", () => {
    it("starts with nothing on the card", () => {
      const panel = new TriggerPanel(new FakeApi());
      expect(panel.cable).toBeNull();
      expect(panel.cableNoAnswer).toBe(false);
      expect(panel.cableBusy).toBe(false);
    });

    it("shows what the cable answered, as read", async () => {
      const { panel } = await inSession();
      await panel.readCable();
      expect(panel.cable).toEqual({ cable: CABLE, source: "read" });
      expect(panel.cableNoAnswer).toBe(false);
      expect(panel.error).toBeNull();
    });

    it("is busy while the read runs", async () => {
      const { api, panel } = await inSession();
      const release = heldRead(api);
      const read = panel.readCable();
      expect(panel.cableBusy).toBe(true);
      release({ kind: "found", cable: CABLE });
      await read;
      expect(panel.cableBusy).toBe(false);
    });
  });

  describe("AC-AXHK2P.2: the read and nothing else", () => {
    it("never opens, enters, lists, detects or closes", async () => {
      const { api, panel } = await inSession();
      await panel.readCable();
      expect(api.calls).toEqual(["cable read"]);
    });

    it("with no session it calls nothing", async () => {
      const api = new FakeApi();
      const panel = new TriggerPanel(api);
      await panel.readCable();
      expect(api.calls).toEqual([]);
      expect(panel.cable).toBeNull();
      expect(panel.cableNoAnswer).toBe(false);
    });

    it("out of step it calls nothing and the power-cycle error stays", async () => {
      const { api, panel } = await inSession();
      const outOfSync = { kind: "out_of_sync", message: "Power-cycle it.", hint: null };
      api.cableRead = async () => {
        api.calls.push("cable read");
        throw outOfSync;
      };
      await panel.readCable();
      expect(panel.requestsDisabled).toBe(true);
      expect(panel.error).toEqual(outOfSync);
      await panel.readCable();
      expect(api.calls).toEqual(["cable read"]);
      expect(panel.error).toEqual(outOfSync);
    });

    it("a second press while one runs does nothing", async () => {
      const { api, panel } = await inSession();
      const release = heldRead(api);
      const first = panel.readCable();
      await panel.readCable();
      await panel.readCable();
      expect(api.calls).toEqual(["cable read"]);
      release({ kind: "found", cable: CABLE });
      await first;
      expect(api.calls).toEqual(["cable read"]);
    });

    it("a refusal shows as the error, with no retry and nothing on the card", async () => {
      const { api, panel } = await inSession();
      const refusal = { kind: "usage", message: "Enter PD first.", hint: null };
      api.cableRead = async () => {
        api.calls.push("cable read");
        throw refusal;
      };
      await panel.readCable();
      expect(api.calls).toEqual(["cable read"]);
      expect(panel.error).toEqual(refusal);
      expect(panel.cable).toBeNull();
      expect(panel.cableNoAnswer).toBe(false);
      expect(panel.cableBusy).toBe(false);
    });

    it("a late answer from a session that ended is dropped", async () => {
      const { api, panel } = await inSession();
      const release = heldRead(api);
      const read = panel.readCable();
      await panel.leave();
      release({ kind: "found", cable: CABLE });
      await read;
      expect(panel.cable).toBeNull();
      expect(panel.cableBusy).toBe(false);
    });
  });

  describe("AC-AXHK2P.4: no answer", () => {
    it("says so once and asks again only on the next press", async () => {
      const { api, panel } = await inSession();
      api.cableAnswer = { kind: "no_answer" };
      await panel.readCable();
      expect(panel.cableNoAnswer).toBe(true);
      expect(panel.cable).toBeNull();
      expect(api.calls).toEqual(["cable read"]);
    });

    it("an old answer does not stay next to it", async () => {
      const { api, panel } = await inSession();
      await panel.readCable();
      api.cableAnswer = { kind: "no_answer" };
      await panel.readCable();
      expect(panel.cable).toBeNull();
      expect(panel.cableNoAnswer).toBe(true);
    });

    it("a later answer clears it", async () => {
      const { api, panel } = await inSession();
      api.cableAnswer = { kind: "no_answer" };
      await panel.readCable();
      api.cableAnswer = { kind: "found", cable: CABLE };
      await panel.readCable();
      expect(panel.cableNoAnswer).toBe(false);
      expect(panel.cable?.source).toBe("read");
    });
  });

  describe('a read ends "don\'t ask again"', () => {
    it("takes the status down from the backend", async () => {
      const api = new FakeApi();
      const panel = new TriggerPanel(api);
      await panel.detect();
      api.armedNow = true;
      await panel.refreshArmed();
      expect(panel.armed).toBe(true);
      api.cableRead = async () => {
        api.armedNow = false;
        return { kind: "no_answer" };
      };
      await panel.readCable();
      expect(panel.armed).toBe(false);
    });
  });

  describe("AC-AXHK2P.5: the vendor", () => {
    it("shows a listed vendor by name", () => {
      expect(cableVendorText({ ...CABLE, vendor: "Apple", vid: 0x05ac })).toBe("Apple");
    });

    it("shows an unlisted vendor as its hex VID", () => {
      expect(cableVendorText({ ...CABLE, vendor: null, vid: 0x1234 })).toBe("0x1234");
      expect(cableVendorText({ ...CABLE, vendor: null, vid: 0x00ab })).toBe("0x00AB");
    });
  });

  describe("AC-AXHK2P.6: the identity a capture holds", () => {
    it("with no session it shows the capture's identity and sends nothing", async () => {
      const api = new FakeApi();
      api.cableInCapture = CABLE;
      const panel = new TriggerPanel(api);
      await panel.loadCaptureCable();
      expect(panel.cable).toEqual({ cable: CABLE, source: "capture" });
      expect(api.calls).toEqual(["cable seen"]);
    });

    it("a capture with no identity leaves the card empty", async () => {
      const api = new FakeApi();
      const panel = new TriggerPanel(api);
      await panel.loadCaptureCable();
      expect(panel.cable).toBeNull();
      expect(panel.cableNoAnswer).toBe(false);
    });

    it("a capture cleared since then takes it away", async () => {
      const api = new FakeApi();
      api.cableInCapture = CABLE;
      const panel = new TriggerPanel(api);
      await panel.loadCaptureCable();
      api.cableInCapture = null;
      await panel.loadCaptureCable();
      expect(panel.cable).toBeNull();
    });

    it("with a session open it does not cover what the cable answers", async () => {
      const { api, panel } = await inSession();
      api.cableInCapture = { ...CABLE, vid: 0x18d1 };
      await panel.readCable();
      await panel.loadCaptureCable();
      expect(panel.cable).toEqual({ cable: CABLE, source: "read" });
      expect(api.calls).not.toContain("cable seen");
    });

    it("a read replaces the capture's identity", async () => {
      const api = new FakeApi();
      api.cableInCapture = { ...CABLE, vid: 0x18d1 };
      const panel = new TriggerPanel(api);
      await panel.loadCaptureCable();
      await panel.detect();
      await panel.readCable();
      expect(panel.cable).toEqual({ cable: CABLE, source: "read" });
    });

    it("no answer replaces the capture's identity", async () => {
      const api = new FakeApi();
      api.cableInCapture = CABLE;
      api.cableAnswer = { kind: "no_answer" };
      const panel = new TriggerPanel(api);
      await panel.loadCaptureCable();
      await panel.detect();
      await panel.readCable();
      expect(panel.cable).toBeNull();
      expect(panel.cableNoAnswer).toBe(true);
    });

    it("the end of a session empties the card", async () => {
      const { api, panel } = await inSession();
      await panel.readCable();
      await panel.leave();
      expect(panel.cable).toBeNull();
      api.cableAnswer = { kind: "no_answer" };
      await panel.detect();
      await panel.readCable();
      await panel.leave();
      expect(panel.cableNoAnswer).toBe(false);
    });

    it("a disconnected meter empties the card", async () => {
      const { panel } = await inSession();
      await panel.readCable();
      panel.handleEvent({ type: "status", data: { state: "disconnected", error: null } });
      expect(panel.cable).toBeNull();
      expect(panel.cableNoAnswer).toBe(false);
    });
  });
});

describe("AXHK2P review: the card never outlives what it describes", () => {
  it("a read that fails leaves no older reading on the card", async () => {
    const api = new FakeApi();
    const panel = new TriggerPanel(api);
    await panel.detect();
    api.cableRead = async () => ({ kind: "no_answer" });
    await panel.readCable();
    expect(panel.cableNoAnswer).toBe(true);
    api.cableRead = async () => {
      throw { kind: "usage", message: "no", hint: null };
    };
    await panel.readCable();
    expect(panel.cableNoAnswer).toBe(false);
    expect(panel.cable).toBeNull();
  });

  it("another meter on screen drops the card and reads the capture again", async () => {
    const api = new FakeApi();
    const panel = new TriggerPanel(api);
    const shown = {
      vid: 1,
      pid: null,
      active: false,
      max_voltage_v: 20,
      max_current_ma: 3000,
      epr_capable: false,
      speed: null,
      vendor: null,
    };
    api.cableSeen = async () => shown;
    await panel.loadCaptureCable();
    expect(panel.cable?.source).toBe("capture");
    api.cableSeen = async () => null;
    await panel.meterChanged();
    expect(panel.cable).toBeNull();
  });
});

// ---- AC-AXHK2N.9 to .13: a charger that goes away, and a view with nothing dead on it ----

const GONE_REFUSAL: UiError = {
  kind: "usage",
  message: "The charger went away. Press Detect.",
  hint: null,
};

/** Lets the answers of the promises already settled land. */
async function settled(): Promise<void> {
  await new Promise((resolve) => setTimeout(resolve));
}

/** A promise settled from outside. */
function held<T>(): { promise: Promise<T>; resolve(value: T): void } {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((r) => (resolve = r));
  return { promise, resolve };
}

/** The backend after a collapse: it reports it, and refuses what names an offer. */
function collapse(api: ArmedApi): void {
  api.goneNow = true;
  api.armedNow = false;
  api.risk = async () => {
    throw GONE_REFUSAL;
  };
  api.request = async () => {
    throw GONE_REFUSAL;
  };
}

/** The panel of an armed session, after a batch at 0 V saw the collapse. */
async function collapsed(): Promise<{ api: ArmedApi; panel: TriggerPanel }> {
  const { api, panel } = await armedPanel();
  collapse(api);
  panel.handleEvent(batch(0));
  await settled();
  return { api, panel };
}

function nothingListed(panel: TriggerPanel): void {
  expect(panel.sections.map((s) => s.state)).toEqual(TRIGGER_PROTOCOLS.map(() => "pending"));
  expect(panel.sections.every((s) => s.section === null)).toBe(true);
  expect(panel.detection).toBeNull();
}

describe("AC-AXHK2N.9: VBUS collapses", () => {
  it("the level is 2 V", () => {
    expect(VBUS_GONE_V).toBe(2);
  });

  it("a batch below it after a request asks the backend, and its yes ends the status", async () => {
    const { api, panel } = await armedPanel();
    collapse(api);
    panel.handleEvent(batch(1.99));
    await settled();
    expect(api.goneAsked).toBeGreaterThan(0);
    expect(panel.armed).toBe(false);
    expect(panel.chargerGone).toBe(true);
  });

  it("the backend's no changes nothing", async () => {
    const { api, panel } = await armedPanel();
    panel.handleEvent(batch(0));
    await settled();
    expect(api.goneAsked).toBeGreaterThan(0);
    expect(panel.chargerGone).toBe(false);
    expect(panel.armed).toBe(true);
    expect(panel.reply).not.toBeNull();
    expect(panel.sections[0]?.state).toBe("supported");
  });

  it("a dip to 4.5 V during a voltage change asks nothing", async () => {
    const { api, panel } = await armedPanel();
    panel.handleEvent(batch(4.5));
    await settled();
    expect(api.goneAsked).toBe(0);
    expect(panel.armed).toBe(true);
  });

  it("exactly 2 V asks nothing", async () => {
    const { api, panel } = await armedPanel();
    panel.handleEvent(batch(VBUS_GONE_V));
    await settled();
    expect(api.goneAsked).toBe(0);
  });

  it("0 V before any request asks nothing", async () => {
    const { api, panel } = await rememberPanel();
    panel.handleEvent(batch(0));
    await settled();
    expect(api.goneAsked).toBe(0);
    expect(panel.chargerGone).toBe(false);
    expect(panel.sections[0]?.state).toBe("supported");
  });

  it("0 V after Back to 5 V counts, since that request was accepted too", async () => {
    const { api, panel } = await armedPanel();
    await panel.backTo5v();
    collapse(api);
    panel.handleEvent(batch(0));
    await settled();
    expect(panel.chargerGone).toBe(true);
  });

  it("a backend that cannot answer changes nothing and shows no error", async () => {
    const { api, panel } = await armedPanel();
    api.chargerGone = async () => {
      throw { kind: "internal", message: "gone", hint: null };
    };
    panel.handleEvent(batch(0));
    await settled();
    expect(panel.chargerGone).toBe(false);
    expect(panel.armed).toBe(true);
    expect(panel.error).toBeNull();
  });

  it("an answer that lands after leaving the view is dropped", async () => {
    const { api, panel } = await armedPanel();
    const answer = held<boolean>();
    api.chargerGone = () => answer.promise;
    panel.handleEvent(batch(0));
    await panel.leave();
    answer.resolve(true);
    await settled();
    expect(panel.chargerGone).toBe(false);
  });
});

describe("AC-AXHK2N.10: the collapse forgets the charger", () => {
  it("drops the offers, the detection and the request's result", async () => {
    const { panel } = await collapsed();
    nothingListed(panel);
    expect(panel.reply).toBeNull();
    expect(panel.requestedMv).toBeNull();
  });

  it("drops the cable it read from that charger", async () => {
    const { api, panel } = await armedPanel();
    await panel.readCable();
    expect(panel.cable).not.toBeNull();
    collapse(api);
    panel.handleEvent(batch(0));
    await settled();
    expect(panel.cable).toBeNull();
  });

  it("closes a dialog still open on an offer that is gone", async () => {
    const { api, panel } = await rememberPanel();
    await at9V(panel);
    await panel.ask(PD_15V);
    expect(panel.pending).not.toBeNull();
    collapse(api);
    panel.handleEvent(batch(0));
    await settled();
    expect(panel.pending).toBeNull();
  });

  it("leaves the session open and calls nothing else", async () => {
    const { api, panel } = await armedPanel();
    collapse(api);
    const calls = [...api.calls];
    panel.handleEvent(batch(0));
    await settled();
    expect(panel.sessionOpen).toBe(true);
    expect(api.calls).toEqual(calls);
  });

  it("a request after it is refused: nothing is sent and the refusal shows", async () => {
    const { api, panel } = await collapsed();
    const sent = api.sent.length;
    await panel.ask(PD_9V);
    expect(api.sent).toHaveLength(sent);
    expect(panel.pending).toBeNull();
    expect(panel.error?.message).toMatch(/Press Detect/);
  });

  it("does not ask again once it knows", async () => {
    const { api, panel } = await collapsed();
    const asked = api.goneAsked;
    panel.handleEvent(batch(0));
    await settled();
    expect(api.goneAsked).toBe(asked);
  });

  it("Detect clears it as it starts, and lists the charger again", async () => {
    const { api, panel } = await collapsed();
    api.goneNow = false;
    const answer = held<Detection>();
    api.detect = () => answer.promise;
    const detecting = panel.detect();
    await settled();
    expect(panel.chargerGone).toBe(false);
    answer.resolve(DETECTION);
    await detecting;
    expect(panel.detection).toEqual(DETECTION);
    expect(panel.sections[0]?.state).toBe("supported");
  });
});

describe("AC-AXHK2N.11: the end of a session leaves nothing that does nothing", () => {
  it("leaving the view drops the offers and the detection", async () => {
    const { panel } = await detected();
    await panel.leave();
    nothingListed(panel);
  });

  it("a disconnect drops them too", async () => {
    const { panel } = await detected();
    panel.handleEvent({ type: "status", data: { state: "disconnected" } } as BackendEvent);
    nothingListed(panel);
  });

  it("a collapse is forgotten with the session", async () => {
    const { panel } = await collapsed();
    await panel.leave();
    expect(panel.chargerGone).toBe(false);
  });

  it("the next Detect lists them again", async () => {
    const { panel } = await detected();
    await panel.leave();
    await panel.detect();
    expect(panel.detection).toEqual(DETECTION);
    expect(panel.sections[0]?.state).toBe("supported");
  });

  it("asking with no session says to press Detect and calls nothing", async () => {
    const { api, panel } = await detected();
    await panel.leave();
    api.calls = [];
    await panel.ask(PD_5V);
    expect(api.calls).toEqual([]);
    expect(panel.error?.kind).toBe("usage");
    expect(panel.error?.message).toMatch(/Press Detect/);
  });

  it("asking on a page that never detected says so too", async () => {
    const api = new FakeApi();
    const panel = new TriggerPanel(api);
    await panel.ask(PD_9V);
    expect(api.calls).toEqual([]);
    expect(panel.error?.message).toMatch(/Press Detect/);
  });
});

describe("AC-AXHK2N.13: a request in flight, and Detect", () => {
  /** A backend whose requests wait for `gate`. */
  function heldRequests(api: ArmedApi): { release(): void } {
    const gate = held<void>();
    const send = api.request.bind(api);
    api.request = async (...args) => {
      await gate.promise;
      return send(...args);
    };
    return { release: () => gate.resolve() };
  }

  it("is not requesting before anything was sent", async () => {
    const { panel } = await rememberPanel();
    expect(panel.requesting).toBe(false);
  });

  it("a request sent at once shows until its answer", async () => {
    const { api, panel } = await rememberPanel();
    const { release } = heldRequests(api);
    const asking = panel.ask(PD_5V);
    await settled();
    expect(panel.requesting).toBe(true);
    release();
    await asking;
    expect(panel.requesting).toBe(false);
  });

  it("a dialog waiting for the user is not a request in flight", async () => {
    const { panel } = await rememberPanel();
    await panel.ask(PD_9V);
    expect(panel.pending).not.toBeNull();
    expect(panel.requesting).toBe(false);
  });

  it("a confirmed send shows until its answer", async () => {
    const { api, panel } = await rememberPanel();
    const { release } = heldRequests(api);
    await panel.ask(PD_9V);
    const confirming = panel.confirm();
    await settled();
    expect(panel.requesting).toBe(true);
    release();
    await confirming;
    expect(panel.requesting).toBe(false);
  });

  it("a failed request stops showing", async () => {
    const { api, panel } = await rememberPanel();
    api.request = async () => {
      throw { kind: "timeout", message: "No answer.", hint: null };
    };
    await panel.ask(PD_5V);
    expect(panel.requesting).toBe(false);
    expect(panel.error?.kind).toBe("timeout");
  });

  it("a request still out when the view is left stops showing once it ends", async () => {
    const { api, panel } = await rememberPanel();
    const { release } = heldRequests(api);
    const asking = panel.ask(PD_5V);
    await settled();
    await panel.leave();
    release();
    await asking;
    expect(panel.requesting).toBe(false);
  });

  it("Detect takes the armed status down as it starts", async () => {
    const { api, panel } = await armedPanel();
    const answer = held<Detection>();
    api.detect = () => {
      api.armedNow = false;
      return answer.promise;
    };
    const detecting = panel.detect();
    expect(panel.armed).toBe(false);
    answer.resolve(DETECTION);
    await detecting;
    expect(panel.armed).toBe(false);
  });

  it("an armed answer from before Detect does not bring the status back", async () => {
    const { api, panel } = await armedPanel();
    const answer = held<Detection>();
    api.detect = () => answer.promise;
    const reading = panel.refreshArmed();
    const detecting = panel.detect();
    await reading;
    expect(panel.armed).toBe(false);
    answer.resolve(DETECTION);
    await detecting;
  });
});

describe("AC-AXHK2N.13: a second press while the first is out", () => {
  it("sends one request", async () => {
    const api = new FakeApi();
    const panel = new TriggerPanel(api);
    await panel.detect();
    const send = vi.spyOn(api, "request");
    await Promise.all([panel.ask(PD_15V), panel.ask(PD_15V)]);
    await panel.confirm();
    await panel.confirm();
    expect(send).toHaveBeenCalledTimes(1);
  });
});

describe("AC-AXHK2N.10: a refused press re-reads the backend", () => {
  it("takes the page down when the backend knows the charger went away", async () => {
    const api = new FakeApi();
    const panel = new TriggerPanel(api);
    await panel.detect();
    await panel.ask(PD_15V);
    await panel.confirm();
    api.risk = async () => {
      throw { kind: "usage", message: "The charger went away. Press Detect.", hint: null };
    };
    api.goneNow = true;
    await panel.ask(PD_15V);
    await vi.waitFor(() => expect(panel.chargerGone).toBe(true));
  });
});

describe("AC-AXHK2N.14: Detect holds every control that sends", () => {
  /** A panel whose next Detect waits until `finish`. */
  async function detectingPanel(): Promise<{
    api: ArmedApi;
    panel: TriggerPanel;
    finish(): Promise<void>;
  }> {
    const { api, panel } = await rememberPanel();
    const answer = held<Detection>();
    api.detect = () => {
      api.calls.push("detect");
      return answer.promise;
    };
    const detecting = panel.detect();
    await settled();
    return {
      api,
      panel,
      finish: async () => {
        answer.resolve(DETECTION);
        await detecting;
        await settled();
      },
    };
  }

  /** What went to the backend that could reach the meter, after the first `from` calls. */
  function sends(api: FakeApi, from: number): string[] {
    return api.calls
      .slice(from)
      .filter((c) => c === "risk" || c.startsWith("request") || c === "back" || c === "cable read");
  }

  it("says Detect is running from its start to its end, and only then", async () => {
    const { panel } = await rememberPanel();
    expect(panel.detecting).toBe(false);
    const api = new ArmedApi();
    const fresh = new TriggerPanel(api);
    const answer = held<Detection>();
    api.detect = () => answer.promise;
    const detecting = fresh.detect();
    expect(fresh.detecting).toBe(true);
    answer.resolve(DETECTION);
    await detecting;
    expect(fresh.detecting).toBe(false);
  });

  it("a Detect that fails stops saying so", async () => {
    const { api, panel } = await rememberPanel();
    api.detect = async () => {
      throw { kind: "timeout", message: "No answer.", hint: null };
    };
    const detecting = panel.detect();
    expect(panel.detecting).toBe(true);
    await detecting;
    expect(panel.detecting).toBe(false);
  });

  it("an offer pressed during Detect asks nothing and opens no dialog after it", async () => {
    const { api, panel, finish } = await detectingPanel();
    const from = api.calls.length;
    await panel.ask(PD_9V);
    expect(panel.pending).toBeNull();
    await finish();
    expect(panel.pending).toBeNull();
    expect(sends(api, from)).toEqual([]);
    expect(api.sent).toEqual([]);
  });

  it("a press the backend answers only after Detect ended opens no dialog", async () => {
    // The meter's session answers in turn: the risk of a press waits behind Detect.
    const { api, panel, finish } = await detectingPanel();
    const risk = held<RiskClass>();
    api.risk = () => {
      api.calls.push("risk");
      return risk.promise;
    };
    const asking = panel.ask(PD_9V);
    await finish();
    risk.resolve("above_5v_rememberable");
    await asking;
    await settled();
    expect(panel.pending).toBeNull();
    expect(api.sent).toEqual([]);
  });

  it("a 5 V offer pressed during Detect sends nothing", async () => {
    const { api, panel, finish } = await detectingPanel();
    const from = api.calls.length;
    await panel.ask(PD_5V);
    await finish();
    expect(sends(api, from)).toEqual([]);
    expect(api.sent).toEqual([]);
    expect(panel.reply).toBeNull();
  });

  it("Back to 5 V pressed during Detect sends nothing", async () => {
    const { api, panel, finish } = await detectingPanel();
    const from = api.calls.length;
    await panel.backTo5v();
    await finish();
    expect(sends(api, from)).toEqual([]);
  });

  it("Read cable pressed during Detect sends nothing", async () => {
    const { api, panel, finish } = await detectingPanel();
    const from = api.calls.length;
    await panel.readCable();
    await finish();
    expect(sends(api, from)).toEqual([]);
    expect(panel.cable).toBeNull();
  });

  it("a press after Detect ended is answered as before", async () => {
    const { api, panel, finish } = await detectingPanel();
    await finish();
    await panel.ask(PD_9V);
    expect(panel.pending).not.toBeNull();
    await panel.confirm();
    expect(api.sent.map((s) => s.request)).toEqual([PD_9V]);
  });
});

describe("AC-AXHK2N.15: leaving the tab names itself to the backend", () => {
  it("leaving returns to 5 V as a leave", async () => {
    const { api, panel } = await detected();
    const back = vi.spyOn(api, "backTo5v");
    await panel.leave();
    expect(back).toHaveBeenCalledTimes(1);
    expect(back.mock.calls[0]).toEqual([true]);
  });

  it("a press of Back to 5 V is no leave", async () => {
    const { api, panel } = await detected();
    const back = vi.spyOn(api, "backTo5v");
    await panel.backTo5v();
    expect(back).toHaveBeenCalledTimes(1);
    expect(back.mock.calls[0]?.[0]).not.toBe(true);
  });
});

describe("AC-AXHK2N.14 review: one Detect at a time", () => {
  /** `api.detect` answers only when the returned function is called. */
  function heldDetect(api: FakeApi): (answer: Detection) => void {
    const answer = held<Detection>();
    api.detect = () => {
      api.calls.push("detect");
      return answer.promise;
    };
    return (d) => answer.resolve(d);
  }

  it("a second Detect while one runs returns at once and sends nothing", async () => {
    const { api, panel } = await rememberPanel();
    const finish = heldDetect(api);
    const first = panel.detect();
    await settled();
    const from = api.calls.length;
    await panel.detect();
    expect(api.calls.slice(from)).toEqual([]);
    expect(panel.detecting).toBe(true);
    finish(DETECTION);
    await first;
    expect(panel.detecting).toBe(false);
  });

  it("leave during Detect, come back, a second Detect: the first one's end does not re-enable presses", async () => {
    const { api, panel } = await rememberPanel();
    const finishFirst = heldDetect(api);
    const first = panel.detect();
    await settled();
    await panel.leave();
    expect(panel.detecting).toBe(false);
    const finishSecond = heldDetect(api);
    const second = panel.detect();
    await settled();
    expect(panel.detecting).toBe(true);
    const from = api.calls.length;
    finishFirst(DETECTION);
    await first;
    await settled();
    // The first one stopped: no listing, and Detect still holds the presses.
    expect(api.calls.slice(from)).toEqual([]);
    expect(panel.detecting).toBe(true);
    await panel.ask(PD_9V);
    expect(panel.pending).toBeNull();
    expect(api.calls.slice(from)).toEqual([]);
    finishSecond(DETECTION);
    await second;
    expect(panel.detecting).toBe(false);
    expect(api.calls.filter((c) => c === "list pd")).toHaveLength(2);
  });

  it("a session end during Detect stops it and clears detecting", async () => {
    const { api, panel } = await rememberPanel();
    const finish = heldDetect(api);
    const running = panel.detect();
    await settled();
    panel.handleEvent({
      type: "status",
      data: { state: "disconnected", error: null },
    } as BackendEvent);
    expect(panel.detecting).toBe(false);
    const from = api.calls.length;
    finish(DETECTION);
    await running;
    expect(api.calls.slice(from).filter((c) => c.startsWith("list"))).toEqual([]);
    expect(panel.detecting).toBe(false);
  });
});

describe("AC-AXHK2N.14 review: the console during Detect", () => {
  class ConsoleFake implements ConsoleApi {
    sent: string[] = [];
    risk = held<RiskClass>();
    async messageRisk(): Promise<RiskClass> {
      return this.risk.promise;
    }
    async messageSend(): Promise<PdSendReply> {
      this.sent.push("message");
      return { command: "pd cmd=13", lines: [], cable: null };
    }
    async consoleRisk(): Promise<RiskClass> {
      return this.risk.promise;
    }
    async consoleSend(line: string): Promise<ConsoleLine[]> {
      this.sent.push(line);
      return [];
    }
  }

  /** A detected panel and its console, wired as the page wires them. */
  async function wired(): Promise<{
    api: ArmedApi;
    panel: TriggerPanel;
    con: PdConsole;
    capi: ConsoleFake;
  }> {
    const api = new ArmedApi();
    const capi = new ConsoleFake();
    // eslint-disable-next-line prefer-const
    let con: PdConsole;
    const panel = new TriggerPanel(api, { onDetect: () => con.cancel() });
    con = new PdConsole(capi, () => panel.detecting);
    await panel.detect();
    return { api, panel, con, capi };
  }

  it("a console risk answer that lands during Detect opens no dialog", async () => {
    const { api, panel, con, capi } = await wired();
    const sending = con.sendLine("pd req=2");
    const answer = held<Detection>();
    api.detect = () => answer.promise;
    const detecting = panel.detect();
    await settled();
    capi.risk.resolve("above_5v");
    await sending;
    expect(con.pending).toBeNull();
    answer.resolve(DETECTION);
    await detecting;
    expect(con.pending).toBeNull();
    expect(capi.sent).toEqual([]);
  });

  it("a built message's risk answer during Detect opens no dialog either", async () => {
    const { api, panel, con, capi } = await wired();
    const sending = con.sendMessage({ sop: "sop", msg_type: 13, data_objects: [] });
    const answer = held<Detection>();
    api.detect = () => answer.promise;
    const detecting = panel.detect();
    await settled();
    capi.risk.resolve("above_5v");
    await sending;
    answer.resolve(DETECTION);
    await detecting;
    expect(con.pending).toBeNull();
    expect(capi.sent).toEqual([]);
  });

  it("Detect clears a pending dialog of the panel and of the console", async () => {
    const { panel, con, capi } = await wired();
    capi.risk.resolve("above_5v");
    await con.sendLine("pd req=2");
    expect(con.pending).not.toBeNull();
    await panel.ask(PD_9V);
    expect(panel.pending).not.toBeNull();
    const detecting = panel.detect();
    expect(panel.pending).toBeNull();
    expect(con.pending).toBeNull();
    await detecting;
    expect(capi.sent).toEqual([]);
  });

  it("panel.confirm() sends nothing while Detect runs", async () => {
    const { api, panel } = await wired();
    await panel.ask(PD_9V);
    const pending = panel.pending;
    expect(pending).not.toBeNull();
    const answer = held<Detection>();
    api.detect = () => answer.promise;
    const detecting = panel.detect();
    // A dialog that stayed on screen is pressed anyway.
    panel.pending = pending;
    await panel.confirm();
    answer.resolve(DETECTION);
    await detecting;
    expect(api.sent).toEqual([]);
  });

  it("a console confirm during Detect sends nothing", async () => {
    const { api, panel, con, capi } = await wired();
    capi.risk.resolve("above_5v");
    await con.sendLine("pd req=2");
    const pending = con.pending;
    expect(pending).not.toBeNull();
    const answer = held<Detection>();
    api.detect = () => answer.promise;
    const detecting = panel.detect();
    con.pending = pending;
    await con.confirm();
    answer.resolve(DETECTION);
    await detecting;
    expect(capi.sent).toEqual([]);
  });
});

describe("AC-AXHK2N.14 review: another meter on screen", () => {
  it("drops the pending dialog", async () => {
    const { api, panel } = await rememberPanel();
    await panel.ask(PD_9V);
    expect(panel.pending).not.toBeNull();
    await panel.meterChanged();
    expect(panel.pending).toBeNull();
    await panel.confirm();
    expect(api.sent).toEqual([]);
  });

  it("stops a running Detect, so nothing later goes to the other meter", async () => {
    const { api, panel } = await rememberPanel();
    const answer = held<Detection>();
    api.detect = () => {
      api.calls.push("detect");
      return answer.promise;
    };
    const running = panel.detect();
    await settled();
    await panel.meterChanged();
    expect(panel.detecting).toBe(false);
    const from = api.calls.length;
    answer.resolve(DETECTION);
    await running;
    await settled();
    expect(api.calls.slice(from).filter((c) => c.startsWith("list"))).toEqual([]);
  });

  it("a press whose answer lands after the switch opens no dialog", async () => {
    const { api, panel } = await rememberPanel();
    const risk = held<RiskClass>();
    api.risk = () => {
      api.calls.push("risk");
      return risk.promise;
    };
    const asking = panel.ask(PD_9V);
    await settled();
    await panel.meterChanged();
    risk.resolve("above_5v_rememberable");
    await asking;
    expect(panel.pending).toBeNull();
    expect(api.sent).toEqual([]);
  });
});
