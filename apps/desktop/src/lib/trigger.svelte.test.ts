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
  confirmationText,
  detectionChecks,
  deviates,
  requestVoltageMv,
  type TriggerApi,
} from "./trigger.svelte";

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

const PD_9V: TriggerRequest = { protocol: "pd", position: 2, voltage_mv: null, current_ma: null };
const PD_5V: TriggerRequest = { protocol: "pd", position: 1, voltage_mv: null, current_ma: null };

/** A scripted backend: PD and QC supported, the rest not; records every call. */
class FakeApi implements TriggerApi {
  calls: string[] = [];
  sent: { request: TriggerRequest; confirmed: boolean }[] = [];
  supported: TriggerProtocol[] = ["pd", "qc"];

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
  async backTo5v(): Promise<void> {
    this.calls.push("back");
  }
  async close(): Promise<void> {
    this.calls.push("close");
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
