/**
 * FR DGSQ6S (AC .8): the trigger view's request controls for the non-PD protocols.
 * QC 2.0 offers 5 / 9 / 12 / 20 V buttons and QC 3.0 a 3.6-20 V control in 200 mV
 * steps; FCP, AFC and SFCP offer 5 / 9 / 12 V buttons; SCP, VFCP and TFCP take a
 * voltage and a current inside the decoded limits, and UFCS does the same per
 * capability (by its index). Every one goes through the same confirmation above 5 V,
 * live VBUS check and Back to 5 V as the PD section.
 *
 * Contract (exported from `trigger.svelte.ts`):
 * - `LEGACY_FIXED_VOLTS`: the fixed-voltage buttons per protocol, in volts;
 * - `QC3_RANGE`: `{ minMv, maxMv, stepMv }` of the QC 3.0 control;
 * - `clampToLimits(limits, voltageMv, currentMa)`: `{ voltage_mv, current_ma }` kept
 *   inside the block's voltage and current range and, when it has one, its power;
 * - `limitsRequest(protocol, limits, voltageMv, currentMa)`: the request for an SCP /
 *   VFCP / TFCP / UFCS control, clamped as above; UFCS addresses the capability by
 *   the block's index.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type {
  BackendEvent,
  Detection,
  ProtocolSection,
  RiskClass,
  TriggerLimits,
  TriggerProtocol,
  TriggerReply,
  TriggerRequest,
} from "./backend";
import {
  DEVIATION_DELAY_MS,
  LEGACY_FIXED_VOLTS,
  QC3_RANGE,
  TriggerPanel,
  clampToLimits,
  limitsRequest,
  requestVoltageMv,
  type TriggerApi,
} from "./trigger.svelte";

const SCP_LIMITS: TriggerLimits = {
  index: null,
  min_voltage_mv: 3400,
  max_voltage_mv: 11000,
  min_current_ma: 500,
  max_current_ma: 5000,
  max_power_mw: null,
};

const UFCS_1: TriggerLimits = {
  index: 1,
  min_voltage_mv: 3400,
  max_voltage_mv: 11000,
  min_current_ma: 500,
  max_current_ma: 5000,
  max_power_mw: 40000,
};

const UFCS_2: TriggerLimits = {
  index: 2,
  min_voltage_mv: 3400,
  max_voltage_mv: 21000,
  min_current_ma: 500,
  max_current_ma: 3000,
  max_power_mw: 63000,
};

function sectionOf(protocol: TriggerProtocol): ProtocolSection {
  const limits =
    protocol === "ufcs"
      ? [UFCS_1, UFCS_2]
      : protocol === "scp" || protocol === "vfcp" || protocol === "tfcp"
        ? [SCP_LIMITS]
        : [];
  return { protocol, supported: true, offers: [], limits, settings: null };
}

function mvOf(r: TriggerRequest): number {
  if (r.protocol === "pd") return r.voltage_mv ?? 5000;
  if ("volts" in r) return r.volts * 1000;
  return r.voltage_mv;
}

/** A scripted charger that speaks every protocol; records what was sent. */
class FakeApi implements TriggerApi {
  calls: string[] = [];
  sent: { request: TriggerRequest; confirmed: boolean }[] = [];

  async open(): Promise<void> {
    this.calls.push("open");
  }
  async detect(): Promise<Detection> {
    this.calls.push("detect");
    return { bc12: "supported", apple: "not-supported", entries: [], complete: true };
  }
  async list(protocol: TriggerProtocol): Promise<ProtocolSection> {
    this.calls.push(`list ${protocol}`);
    return sectionOf(protocol);
  }
  async risk(request: TriggerRequest): Promise<RiskClass> {
    this.calls.push("risk");
    return mvOf(request) > 5000 ? "above_5v" : "at_most_5v";
  }
  async request(request: TriggerRequest, confirmed: boolean): Promise<TriggerReply> {
    this.calls.push(`request ${confirmed}`);
    if (mvOf(request) > 5000 && !confirmed) {
      throw { kind: "usage", message: "Above 5 V needs confirmation.", hint: null };
    }
    this.sent.push({ request, confirmed });
    // Legacy replies carry no `req:` line (like the backend's).
    return {
      command: request.protocol,
      requested_mv: null,
      requested_ma: null,
      delivered_mv: null,
      delivered_ma: null,
      reply: "ready",
    };
  }
  async backTo5v(): Promise<void> {
    this.calls.push("back");
  }
  async close(): Promise<void> {
    this.calls.push("close");
  }
}

function batch(vbus: number): BackendEvent {
  return {
    type: "batch",
    data: {
      samples: [{ t_ms: 0, vbus_v: vbus, ibus_a: 0, cc1_v: 0, cc2_v: 0, dp_v: 0, dm_v: 0 }],
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

describe("AC-DGSQ6S.8: what each non-PD section offers", () => {
  it("QC 2.0 offers 5, 9, 12 and 20 V; FCP, AFC and SFCP 5, 9 and 12 V", () => {
    expect(LEGACY_FIXED_VOLTS.qc).toEqual([5, 9, 12, 20]);
    for (const p of ["fcp", "afc", "sfcp"] as const) {
      expect(LEGACY_FIXED_VOLTS[p]).toEqual([5, 9, 12]);
    }
  });

  it("QC 3.0 runs from 3.6 to 20 V in 200 mV steps", () => {
    expect(QC3_RANGE).toEqual({ minMv: 3600, maxMv: 20000, stepMv: 200 });
  });

  it("the limits-based sections keep their decoded limits after detection", async () => {
    const { panel } = await detected();
    const byProtocol = Object.fromEntries(panel.sections.map((s) => [s.protocol, s]));
    expect(byProtocol.scp?.section?.limits).toEqual([SCP_LIMITS]);
    expect(byProtocol.ufcs?.section?.limits.map((l) => l.index)).toEqual([1, 2]);
  });

  it("knows the voltage of a request for every protocol", () => {
    const cases: [TriggerRequest, number][] = [
      [{ protocol: "qc", volts: 20 }, 20000],
      [{ protocol: "qc3", voltage_mv: 6600 }, 6600],
      [{ protocol: "fcp", volts: 9 }, 9000],
      [{ protocol: "afc", volts: 12 }, 12000],
      [{ protocol: "sfcp", volts: 5 }, 5000],
      [{ protocol: "scp", voltage_mv: 10000, current_ma: 2250 }, 10000],
      [{ protocol: "vfcp", voltage_mv: 4500, current_ma: 4000 }, 4500],
      [{ protocol: "tfcp", voltage_mv: 11000, current_ma: 1000 }, 11000],
      [{ protocol: "ufcs", index: 2, voltage_mv: 21000, current_ma: 3000 }, 21000],
    ];
    for (const [request, mv] of cases) expect(requestVoltageMv(request, [])).toBe(mv);
  });
});

describe("AC-DGSQ6S.8: requests stay inside the decoded limits", () => {
  it("clamps voltage and current to the block's ranges", () => {
    expect(clampToLimits(SCP_LIMITS, 15000, 9000)).toEqual({ voltage_mv: 11000, current_ma: 5000 });
    expect(clampToLimits(SCP_LIMITS, 1000, 0)).toEqual({ voltage_mv: 3400, current_ma: 500 });
    expect(clampToLimits(SCP_LIMITS, 9000, 2000)).toEqual({ voltage_mv: 9000, current_ma: 2000 });
  });

  it("keeps the current under the block's power at that voltage", () => {
    // 40 W at 11 V is 3636 mA.
    const c = clampToLimits(UFCS_1, 11000, 5000);
    expect(c.voltage_mv).toBe(11000);
    expect(c.current_ma).toBeLessThanOrEqual(3636);
    expect(c.current_ma).toBeGreaterThan(3500);
    expect(c.voltage_mv * c.current_ma).toBeLessThanOrEqual(40000 * 1000);
    // Below the power the block's own current limit still holds.
    expect(clampToLimits(UFCS_1, 5000, 5000)).toEqual({ voltage_mv: 5000, current_ma: 5000 });
  });

  it("builds SCP, VFCP and TFCP requests from the limits", () => {
    for (const protocol of ["scp", "vfcp", "tfcp"] as const) {
      expect(limitsRequest(protocol, SCP_LIMITS, 12000, 6000)).toEqual({
        protocol,
        voltage_mv: 11000,
        current_ma: 5000,
      });
    }
  });

  it("lowers the voltage when the power cap would push the current below its minimum", () => {
    const tight = { ...SCP_LIMITS, min_current_ma: 1000, max_power_mw: 5000 };
    const c = clampToLimits(tight, 11000, 1000);
    expect(c.current_ma).toBeGreaterThanOrEqual(tight.min_current_ma);
    expect(c.voltage_mv * c.current_ma).toBeLessThanOrEqual(tight.max_power_mw * 1000);
  });

  it("never guesses a UFCS capability that has no number", () => {
    // Index 0 is no capability (they count from 1): the backend refuses it.
    const r = limitsRequest("ufcs", { ...UFCS_1, index: null }, 9000, 1000);
    expect(r).toMatchObject({ protocol: "ufcs", index: 0 });
  });

  it("addresses UFCS by capability", () => {
    expect(limitsRequest("ufcs", UFCS_2, 21000, 3000)).toEqual({
      protocol: "ufcs",
      index: 2,
      voltage_mv: 21000,
      current_ma: 3000,
    });
    const r = limitsRequest("ufcs", UFCS_1, 20000, 5000);
    expect(r).toMatchObject({ protocol: "ufcs", index: 1, voltage_mv: 11000 });
    expect(r.protocol === "ufcs" && r.current_ma <= 3636).toBe(true);
  });
});

describe("AC-DGSQ6S.8: the same confirmation, live VBUS check and Back to 5 V", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("a QC 9 V request asks for confirmation naming 9 V; nothing is sent yet", async () => {
    const { api, panel } = await detected();
    await panel.ask({ protocol: "qc", volts: LEGACY_FIXED_VOLTS.qc[1]! });
    expect(panel.pending?.voltageMv).toBe(9000);
    expect(panel.pending?.text).toMatch(/\b9(\.0+)? V\b/);
    expect(api.sent).toEqual([]);
    await panel.confirm();
    expect(api.sent).toEqual([{ request: { protocol: "qc", volts: 9 }, confirmed: true }]);
  });

  it("a QC 5 V request is sent at once, unconfirmed", async () => {
    const { api, panel } = await detected();
    await panel.ask({ protocol: "qc", volts: LEGACY_FIXED_VOLTS.qc[0]! });
    expect(panel.pending).toBeNull();
    expect(api.sent).toEqual([{ request: { protocol: "qc", volts: 5 }, confirmed: false }]);
  });

  it("a QC 3.0 request above 5 V names its voltage", async () => {
    const { panel } = await detected();
    await panel.ask({ protocol: "qc3", voltage_mv: QC3_RANGE.minMv + 15 * QC3_RANGE.stepMv });
    expect(panel.pending?.voltageMv).toBe(6600);
    expect(panel.pending?.text).toMatch(/\b6\.6 V\b/);
  });

  it("an SCP request built from the section's limits is confirmed and sent clamped", async () => {
    const { api, panel } = await detected();
    const scp = panel.sections.find((s) => s.protocol === "scp")!.section!;
    await panel.ask(limitsRequest("scp", scp.limits[0]!, 20000, 9000));
    expect(panel.pending?.voltageMv).toBe(11000);
    await panel.confirm();
    expect(api.sent).toEqual([
      { request: { protocol: "scp", voltage_mv: 11000, current_ma: 5000 }, confirmed: true },
    ]);
  });

  it("a UFCS request names the capability from the section", async () => {
    const { api, panel } = await detected();
    const ufcs = panel.sections.find((s) => s.protocol === "ufcs")!.section!;
    await panel.ask(limitsRequest("ufcs", ufcs.limits[1]!, 21000, 3000));
    expect(panel.pending?.voltageMv).toBe(21000);
    await panel.confirm();
    expect(api.sent.at(-1)?.request).toEqual({
      protocol: "ufcs",
      index: 2,
      voltage_mv: 21000,
      current_ma: 3000,
    });
  });

  it("a legacy reply without `req:` still feeds the live VBUS check", async () => {
    const { panel } = await detected();
    await panel.ask({ protocol: "fcp", volts: 12 });
    await panel.confirm();
    expect(panel.requestedMv).toBe(12000);
    panel.handleEvent(batch(10.5));
    vi.advanceTimersByTime(DEVIATION_DELAY_MS + 1);
    expect(panel.deviation).toBe(true);
    panel.handleEvent(batch(11.9));
    expect(panel.deviation).toBe(false);
  });

  it("Back to 5 V after a legacy request goes through the backend", async () => {
    const { api, panel } = await detected();
    await panel.ask(limitsRequest("tfcp", SCP_LIMITS, 9000, 2000));
    await panel.confirm();
    await panel.backTo5v();
    expect(api.calls.at(-1)).toBe("back");
    expect(panel.requestedMv).toBe(5000);
    expect(panel.sessionOpen).toBe(true);
  });
});
