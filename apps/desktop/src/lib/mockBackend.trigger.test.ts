/**
 * FR DGSQ6S: the mock backend's trigger session, which the Playwright suite drives.
 * Like the backend it detects, lists one section per protocol (unsupported ones
 * marked), classifies requests with the 5 V risk gate and refuses an unconfirmed
 * above-5 V request without sending anything, moves the demo stream's VBUS to what
 * was requested, returns to 5 V on Back to 5 V, close and disconnect, and keeps the
 * queue at 50 SPS or less with the PD capture stopped while the session is open.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { BackendEvent, PdMessageRequest, TriggerRequest } from "./backend";
import { MockBackend, mockModeFromUrl } from "./mockBackend";

const PD_9V: TriggerRequest = { protocol: "pd", position: 2, voltage_mv: null, current_ma: null };
const PD_5V: TriggerRequest = { protocol: "pd", position: 1, voltage_mv: null, current_ma: null };

function lastVbus(events: BackendEvent[]): number {
  for (let i = events.length - 1; i >= 0; i--) {
    const e = events[i]!;
    if (e.type === "batch" && e.data.samples.length > 0) return e.data.samples.at(-1)!.vbus_v;
  }
  return NaN;
}

async function connected(
  rateSps = 50,
  mode: ConstructorParameters<typeof MockBackend>[0] = null,
): Promise<{ backend: MockBackend; events: BackendEvent[] }> {
  const backend = new MockBackend(mode, { pdSynthetic: null });
  const events: BackendEvent[] = [];
  await backend.connect("demo:load-5v-1a", rateSps, (e) => events.push(e));
  return { backend, events };
}

async function opened(
  mode: ConstructorParameters<typeof MockBackend>[0] = null,
): Promise<{ backend: MockBackend; events: BackendEvent[] }> {
  const c = await connected(50, mode);
  await c.backend.triggerOpen();
  return c;
}

beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());

describe("AC-DGSQ6S.1: detection and one section per protocol", () => {
  it("refuses to open a trigger session without a meter", async () => {
    const backend = new MockBackend(null, { pdSynthetic: null });
    await expect(backend.triggerOpen()).rejects.toMatchObject({ kind: "usage" });
  });

  it("detects like the 140 W charger's automatic list", async () => {
    const { backend } = await opened();
    const d = await backend.triggerDetect();
    expect(d.apple).toBe("not-supported");
    expect(d.entries.map((e) => [e.name, e.support])).toEqual([
      ["bc1.2", "supported"],
      ["ufcs", "not-supported"],
      ["vfcp", "not-supported"],
    ]);
    // Detection asks for nothing above 5 V.
    expect(backend.triggerCommands().some((c) => /^(pd req=[2-9]|qc [1-9]\d*V)/.test(c))).toBe(
      false,
    );
  });

  it("marks protocols the charger lacks as not supported", async () => {
    const { backend } = await opened();
    const fcp = await backend.triggerList("fcp");
    expect(fcp.protocol).toBe("fcp");
    expect(fcp.supported).toBe(false);
    expect(fcp.offers).toEqual([]);
    expect((await backend.triggerList("pd")).supported).toBe(true);
    expect((await backend.triggerList("qc")).supported).toBe(true);
  });
});

describe("AC-DGSQ6S.2: PD offers, PPS / AVS ranges and the emulation settings", () => {
  it("lists fixed offers plus a PPS and an AVS range, and the settings", async () => {
    const { backend } = await opened();
    const pd = await backend.triggerList("pd");
    const fixed = pd.offers.filter((o) => o.kind === "fixed").map((o) => o.max_voltage_mv);
    expect(fixed).toEqual(expect.arrayContaining([5000, 9000, 15000, 20000]));
    const pps = pd.offers.find((o) => o.kind === "pps");
    expect(pps).toMatchObject({ min_voltage_mv: 3300, max_voltage_mv: 21000 });
    const avs = pd.offers.find((o) => o.kind === "epr-avs" || o.kind === "spr-avs");
    expect(avs?.max_voltage_mv).toBeGreaterThan(avs?.min_voltage_mv ?? Infinity);
    expect(pd.settings).toEqual({ pd_type: "auto", emarker: "50v-240w-epr", sink: "pps-3a" });
  });
});

describe("AC-DGSQ6S.3: the 5 V risk gate", () => {
  it("classifies requests like the library", async () => {
    const { backend } = await opened();
    await backend.triggerList("pd");
    await backend.triggerList("qc");
    await expect(backend.triggerRisk(PD_5V)).resolves.toBe("at_most_5v");
    await expect(backend.triggerRisk(PD_9V)).resolves.toBe("above_5v_rememberable");
    await expect(backend.triggerRisk({ protocol: "qc", volts: 9 })).resolves.toBe("above_5v");
    await expect(backend.triggerRisk({ protocol: "qc", volts: 5 })).resolves.toBe("at_most_5v");
  });

  it("refuses an unconfirmed above-5 V request and sends nothing", async () => {
    const { backend, events } = await opened();
    await backend.triggerList("pd");
    const before = backend.triggerCommands().length;
    await expect(backend.triggerRequest(PD_9V, false)).rejects.toMatchObject({ kind: "usage" });
    expect(backend.triggerCommands()).toHaveLength(before);
    vi.advanceTimersByTime(500);
    expect(lastVbus(events)).toBeCloseTo(5, 0);
  });
});

describe("AC-DGSQ6S.4: the reply and live VBUS", () => {
  it("a confirmed request answers and moves the stream's VBUS", async () => {
    const { backend, events } = await opened();
    await backend.triggerList("pd");
    const reply = await backend.triggerRequest(PD_9V, true);
    expect(reply.command).toMatch(/^pd req=2/);
    expect(reply.requested_mv).toBe(9000);
    expect(reply.delivered_mv).not.toBeNull();
    expect(backend.triggerCommands().at(-1)).toMatch(/^pd req=2/);
    vi.advanceTimersByTime(2500);
    expect(Math.abs(lastVbus(events) - 9) / 9).toBeLessThanOrEqual(0.05);
  });

  it("`?mock=trigger-sag` leaves VBUS more than 5 % below the request", async () => {
    expect(mockModeFromUrl("?mock=trigger-sag")).toBe("trigger-sag");
    const { backend, events } = await opened("trigger-sag");
    await backend.triggerList("pd");
    await backend.triggerRequest(PD_9V, true);
    vi.advanceTimersByTime(2500);
    expect(Math.abs(lastVbus(events) - 9) / 9).toBeGreaterThan(0.05);
  });
});

describe("AC-DGSQ6S.5: Back to 5 V, close and disconnect", () => {
  it("Back to 5 V sends `pd req=1` after PD and keeps the session", async () => {
    const { backend, events } = await opened();
    await backend.triggerList("pd");
    await backend.triggerRequest(PD_9V, true);
    await backend.triggerBackTo5v();
    expect(backend.triggerCommands().at(-1)).toBe("pd req=1");
    vi.advanceTimersByTime(500);
    expect(lastVbus(events)).toBeCloseTo(5, 0);
    await expect(backend.triggerList("pd")).resolves.toMatchObject({ supported: true });
  });

  it("Back to 5 V sends `reset` after a legacy protocol", async () => {
    const { backend } = await opened();
    await backend.triggerList("qc");
    await backend.triggerRequest({ protocol: "qc", volts: 9 }, true);
    await backend.triggerBackTo5v();
    expect(backend.triggerCommands().at(-1)).toBe("reset");
  });

  it("close returns to 5 V, then sends `pdm close`; the session is gone", async () => {
    const { backend, events } = await opened();
    await backend.triggerList("pd");
    await backend.triggerRequest(PD_9V, true);
    await backend.triggerClose();
    expect(backend.triggerCommands().slice(-2)).toEqual(["pd req=1", "pdm close"]);
    await expect(backend.triggerList("pd")).rejects.toMatchObject({ kind: "usage" });
    vi.advanceTimersByTime(500);
    expect(lastVbus(events)).toBeCloseTo(5, 0);
  });

  it("disconnecting the meter returns to 5 V and closes the session", async () => {
    const { backend } = await opened();
    await backend.triggerList("qc");
    await backend.triggerRequest({ protocol: "qc", volts: 12 }, true);
    await backend.disconnect();
    expect(backend.triggerCommands().slice(-2)).toEqual(["reset", "pdm close"]);
    await expect(backend.triggerDetect()).rejects.toMatchObject({ kind: "usage" });
  });
});

describe("FR DGSQ6S Notes: the queue and the PD capture while triggering [inferred]", () => {
  it("opening the session caps the queue at 50 SPS", async () => {
    const { backend, events } = await connected(1000);
    await backend.triggerOpen();
    const rates = events.flatMap((e) =>
      e.type === "status" && e.data.state === "rate" ? [e.data.rate_sps] : [],
    );
    expect(rates.at(-1)).toBeLessThanOrEqual(50);
  });

  it("opening the session stops a running PD capture, and Start is refused while it is open", async () => {
    const { backend, events } = await connected(50);
    await backend.pdStart();
    await backend.triggerOpen();
    const states = events.flatMap((e) => (e.type === "pd_state" ? [e.data.capturing] : []));
    expect(states.at(-1)).toBe(false);
    await expect(backend.pdStart()).rejects.toMatchObject({ kind: "usage" });
    await backend.triggerClose();
    await expect(backend.pdStart()).resolves.toBeUndefined();
  });
});

/**
 * AC-DGSQ6S.8: `?mock=trigger-all` puts a charger that speaks every protocol behind the
 * demo meter, so the UI suite can drive the non-PD sections. Its decoded limits:
 * SCP 3.4-11 V at 0.5-5 A (no power cap); VFCP and TFCP one block each (index null);
 * UFCS capability #1 3.4-11 V at 0.5-5 A up to 40 W and #2 3.4-21 V at 0.5-3 A up to
 * 63 W. Like the backend it refuses what the protocol or the limits do not allow,
 * before anything is sent.
 */
describe("AC-DGSQ6S.8: `?mock=trigger-all`, a charger that speaks every protocol", () => {
  const ALL = "trigger-all" as unknown as ConstructorParameters<typeof MockBackend>[0];

  it("is a mock mode", () => {
    expect(mockModeFromUrl("?mock=trigger-all")).toBe("trigger-all");
  });

  it("lists every protocol as supported, with the decoded limits", async () => {
    const { backend } = await opened(ALL);
    for (const p of ["pd", "qc", "fcp", "afc", "sfcp", "scp", "vfcp", "tfcp", "ufcs"] as const) {
      expect((await backend.triggerList(p)).supported, p).toBe(true);
    }
    expect((await backend.triggerList("scp")).limits).toEqual([
      {
        index: null,
        min_voltage_mv: 3400,
        max_voltage_mv: 11000,
        min_current_ma: 500,
        max_current_ma: 5000,
        max_power_mw: null,
      },
    ]);
    for (const p of ["vfcp", "tfcp"] as const) {
      const limits = (await backend.triggerList(p)).limits;
      expect(limits).toHaveLength(1);
      expect(limits[0]!.index).toBeNull();
      expect(limits[0]!.max_voltage_mv).toBeGreaterThan(5000);
    }
    expect((await backend.triggerList("ufcs")).limits).toEqual([
      {
        index: 1,
        min_voltage_mv: 3400,
        max_voltage_mv: 11000,
        min_current_ma: 500,
        max_current_ma: 5000,
        max_power_mw: 40000,
      },
      {
        index: 2,
        min_voltage_mv: 3400,
        max_voltage_mv: 21000,
        min_current_ma: 500,
        max_current_ma: 3000,
        max_power_mw: 63000,
      },
    ]);
  });

  it("sends each protocol's command and moves VBUS to it", async () => {
    const { backend, events } = await opened(ALL);
    const cases: [TriggerRequest, string, number][] = [
      [{ protocol: "qc", volts: 20 }, "qc 20V", 20],
      [{ protocol: "qc3", voltage_mv: 6600 }, "qc3 volt=6600", 6.6],
      [{ protocol: "afc", volts: 9 }, "afc 9V", 9],
      [{ protocol: "scp", voltage_mv: 10000, current_ma: 2000 }, "scp volt=10000,cur=2000", 10],
      [
        { protocol: "ufcs", index: 2, voltage_mv: 21000, current_ma: 3000 },
        "ufcs req=2,volt=21000,cur=3000",
        21,
      ],
    ];
    for (const [request, command, volts] of cases) {
      const reply = await backend.triggerRequest(request, true);
      expect(reply.command).toBe(command);
      expect(backend.triggerCommands().at(-1)).toBe(command);
      vi.advanceTimersByTime(500);
      expect(Math.abs(lastVbus(events) - volts) / volts, command).toBeLessThanOrEqual(0.05);
    }
  });

  it("refuses what the protocol or the limits do not allow, and sends nothing", async () => {
    const { backend } = await opened(ALL);
    const refused: TriggerRequest[] = [
      { protocol: "qc", volts: 7 },
      { protocol: "fcp", volts: 20 },
      { protocol: "qc3", voltage_mv: 6700 },
      { protocol: "qc3", voltage_mv: 20200 },
      { protocol: "scp", voltage_mv: 12000, current_ma: 1000 },
      { protocol: "scp", voltage_mv: 9000, current_ma: 6000 },
      { protocol: "ufcs", index: 3, voltage_mv: 9000, current_ma: 1000 },
      // 11 V × 5 A is above capability #1's 40 W.
      { protocol: "ufcs", index: 1, voltage_mv: 11000, current_ma: 5000 },
    ];
    const before = backend.triggerCommands().length;
    for (const r of refused) {
      await expect(backend.triggerRequest(r, true), JSON.stringify(r)).rejects.toMatchObject({
        kind: "usage",
      });
    }
    expect(backend.triggerCommands()).toHaveLength(before);
  });

  it("Back to 5 V sends `reset` after SCP and UFCS", async () => {
    const { backend, events } = await opened(ALL);
    for (const r of [
      { protocol: "scp", voltage_mv: 9000, current_ma: 2000 },
      { protocol: "ufcs", index: 1, voltage_mv: 9000, current_ma: 2000 },
    ] as TriggerRequest[]) {
      await backend.triggerRequest(r, true);
      await backend.triggerBackTo5v();
      expect(backend.triggerCommands().at(-1)).toBe("reset");
      vi.advanceTimersByTime(500);
      expect(lastVbus(events)).toBeCloseTo(5, 0);
    }
  });
});

/** FR AXHK2N, AC .7: the mock follows the backend's "don't ask again" rules. */
describe("AC-AXHK2N.7: the mock's \"don't ask again\"", () => {
  const PD_15V: TriggerRequest = {
    protocol: "pd",
    position: 3,
    voltage_mv: null,
    current_ma: null,
  };
  const PD_20V: TriggerRequest = {
    protocol: "pd",
    position: 4,
    voltage_mv: null,
    current_ma: null,
  };
  const PD_28V: TriggerRequest = {
    protocol: "pd",
    position: 8,
    voltage_mv: null,
    current_ma: null,
  };
  const pps = (voltage_mv: number): TriggerRequest => ({
    protocol: "pd",
    position: 5,
    voltage_mv,
    current_ma: 1000,
  });
  const AVS_18V: TriggerRequest = {
    protocol: "pd",
    position: 9,
    voltage_mv: 18000,
    current_ma: 1000,
  };
  const QC_9V: TriggerRequest = { protocol: "qc", volts: 9 };

  async function ready(): Promise<MockBackend> {
    const { backend } = await opened();
    await backend.triggerList("pd");
    return backend;
  }

  async function armed(): Promise<MockBackend> {
    const backend = await ready();
    await backend.triggerRequest(PD_9V, true, true);
    await expect(backend.triggerArmed()).resolves.toBe(true);
    return backend;
  }

  /** Disarmed: risk asks again and an unconfirmed 9 V is refused, nothing sent. */
  async function expectDisarmed(backend: MockBackend): Promise<void> {
    await expect(backend.triggerArmed()).resolves.toBe(false);
    await expect(backend.triggerRisk(PD_9V)).resolves.toBe("above_5v_rememberable");
    const before = backend.triggerCommands().length;
    await expect(backend.triggerRequest(PD_9V, false)).rejects.toMatchObject({ kind: "usage" });
    expect(backend.triggerCommands()).toHaveLength(before);
  }

  it("classifies what can be remembered", async () => {
    const backend = await ready();
    for (const r of [PD_9V, PD_15V, PD_20V, pps(9000), pps(20000)]) {
      await expect(backend.triggerRisk(r), JSON.stringify(r)).resolves.toBe(
        "above_5v_rememberable",
      );
    }
    for (const r of [PD_28V, pps(20100), pps(21000), AVS_18V, QC_9V]) {
      await expect(backend.triggerRisk(r), JSON.stringify(r)).resolves.toBe("above_5v");
    }
  });

  it("starts unarmed; a plain confirmation does not arm", async () => {
    const backend = await ready();
    await expect(backend.triggerArmed()).resolves.toBe(false);
    await backend.triggerRequest(PD_9V, true);
    await expectDisarmed(backend);
  });

  it("armed, a later request up to 20 V sends unconfirmed", async () => {
    const backend = await armed();
    for (const [r, command] of [
      [PD_15V, "pd req=3"],
      [PD_20V, "pd req=4"],
      [pps(20000), "pd req=5,volt=20000,cur=1000"],
    ] as [TriggerRequest, string][]) {
      await expect(backend.triggerRisk(r)).resolves.toBe("armed");
      await expect(backend.triggerRequest(r, false)).resolves.toMatchObject({ command });
      expect(backend.triggerCommands().at(-1)).toBe(command);
    }
    await expect(backend.triggerArmed()).resolves.toBe(true);
  });

  it("armed, anything else still asks and sends nothing unconfirmed", async () => {
    const backend = await armed();
    await backend.triggerList("qc");
    const before = backend.triggerCommands().length;
    for (const r of [PD_28V, pps(21000), AVS_18V, QC_9V]) {
      await expect(backend.triggerRisk(r), JSON.stringify(r)).resolves.toBe("above_5v");
      await expect(backend.triggerRequest(r, false), JSON.stringify(r)).rejects.toMatchObject({
        kind: "usage",
      });
    }
    await expect(backend.triggerConsoleRisk("pd req=3")).resolves.toBe("above_5v");
    await expect(backend.triggerConsoleSend("pd req=3", false)).rejects.toMatchObject({
      kind: "usage",
    });
    const built = { sop: "sop", msg_type: 2, data_objects: [0x2304b12c] } as const;
    await expect(
      backend.triggerMessageRisk({ ...built, data_objects: [...built.data_objects] }),
    ).resolves.toBe("above_5v");
    expect(backend.triggerCommands()).toHaveLength(before);
  });

  it("refuses to remember a request that does not qualify, and sends nothing", async () => {
    const backend = await ready();
    await backend.triggerList("qc");
    const before = backend.triggerCommands().length;
    for (const r of [PD_28V, pps(21000), AVS_18V, QC_9V]) {
      await expect(backend.triggerRequest(r, true, true), JSON.stringify(r)).rejects.toMatchObject({
        kind: "usage",
      });
    }
    expect(backend.triggerCommands()).toHaveLength(before);
    await expect(backend.triggerArmed()).resolves.toBe(false);
  });

  it("remember without a confirmation arms nothing and sends nothing", async () => {
    const backend = await ready();
    const before = backend.triggerCommands().length;
    await expect(backend.triggerRequest(PD_9V, false, true)).rejects.toMatchObject({
      kind: "usage",
    });
    expect(backend.triggerCommands()).toHaveLength(before);
    await expect(backend.triggerArmed()).resolves.toBe(false);
  });

  it("Clear disarms and sends nothing", async () => {
    const backend = await armed();
    const before = backend.triggerCommands().length;
    await backend.triggerDisarm();
    expect(backend.triggerCommands()).toHaveLength(before);
    await expectDisarmed(backend);
  });

  it("Back to 5 V disarms", async () => {
    const backend = await armed();
    await backend.triggerBackTo5v();
    await expectDisarmed(backend);
  });

  it("a request at or below 5 V disarms", async () => {
    const backend = await armed();
    await backend.triggerRequest(PD_5V, false);
    await expectDisarmed(backend);
  });

  it("Detect disarms", async () => {
    const backend = await armed();
    await backend.triggerDetect();
    await backend.triggerList("pd");
    await expectDisarmed(backend);
  });

  it("any built message disarms", async () => {
    const backend = await armed();
    await backend.triggerMessageSend({ sop: "sop", msg_type: 13, data_objects: [] }, false);
    await expectDisarmed(backend);
  });

  it("a console line that is not read-only disarms", async () => {
    const backend = await armed();
    await backend.triggerConsoleSend("pd hrst", false);
    await expectDisarmed(backend);
  });

  it("a read-only console line stays armed", async () => {
    const backend = await armed();
    await backend.triggerConsoleSend("pdm parm", false);
    await expect(backend.triggerArmed()).resolves.toBe(true);
  });

  it("listing the same offers stays armed", async () => {
    const backend = await armed();
    await backend.triggerList("pd");
    await expect(backend.triggerArmed()).resolves.toBe(true);
  });

  it("closing the session disarms; a new session starts unarmed", async () => {
    const backend = await armed();
    await backend.triggerClose();
    await expect(backend.triggerArmed()).resolves.toBe(false);
    await backend.triggerOpen();
    await backend.triggerList("pd");
    await expectDisarmed(backend);
  });

  it("disconnecting disarms", async () => {
    const backend = await armed();
    await backend.disconnect();
    await expect(backend.triggerArmed()).resolves.toBe(false);
  });

  it("with no session it reports false and clearing does nothing", async () => {
    const backend = new MockBackend(null, { pdSynthetic: null });
    await expect(backend.triggerArmed()).resolves.toBe(false);
    await expect(backend.triggerDisarm()).resolves.toBeUndefined();
  });
});

describe("FR AXHK2P: the mock's cable read follows the backend's rules", () => {
  const READ_LINE = "pd data=018F1001A800FF";
  const READ: PdMessageRequest = {
    sop: "sop_prime",
    msg_type: 15,
    data_objects: [0xff00a801],
  };

  /** A session with PD entered. */
  async function inPd(mode: ConstructorParameters<typeof MockBackend>[0] = null) {
    const { backend } = await opened(mode);
    await backend.triggerList("pd");
    return backend;
  }

  /** `read` adds exactly `expected` to what the charger received. */
  async function sends(
    backend: MockBackend,
    read: () => Promise<unknown>,
    expected: string[],
  ): Promise<void> {
    const before = backend.triggerCommands().length;
    await read().catch(() => {});
    expect(backend.triggerCommands().slice(before)).toEqual(expected);
  }

  it("AC-AXHK2P.8: the demo device answers with a sample cable", async () => {
    const backend = await inPd();
    await expect(backend.triggerCableRead()).resolves.toMatchObject({
      kind: "found",
      cable: {
        vid: 0x050d,
        pid: 0x8254,
        active: false,
        max_voltage_v: 50,
        max_current_ma: 5000,
        epr_capable: true,
        speed: "USB 2.0",
      },
    });
  });

  it("AC-AXHK2P.2: one read sends exactly one line", async () => {
    const backend = await inPd();
    await sends(backend, () => backend.triggerCableRead(), [READ_LINE]);
  });

  it("AC-AXHK2P.2: with no session it is refused", async () => {
    const backend = new MockBackend(null, { pdSynthetic: null });
    await expect(backend.triggerCableRead()).rejects.toMatchObject({ kind: "usage" });
  });

  it("AC-AXHK2P.2: with nothing entered it is refused and sends nothing", async () => {
    const { backend } = await opened();
    await expect(backend.triggerCableRead()).rejects.toMatchObject({ kind: "usage" });
    await sends(backend, () => backend.triggerCableRead(), []);
  });

  it("AC-AXHK2P.2: with a legacy protocol entered last it is refused and sends nothing", async () => {
    const backend = await inPd();
    await backend.triggerList("qc");
    await expect(backend.triggerCableRead()).rejects.toMatchObject({ kind: "usage" });
    await sends(backend, () => backend.triggerCableRead(), []);
  });

  it("AC-AXHK2P.2: after a detection it is refused; a PD offer opens it again", async () => {
    const backend = await inPd();
    await backend.triggerDetect();
    await sends(backend, () => backend.triggerCableRead(), []);
    await backend.triggerList("ufcs");
    await sends(backend, () => backend.triggerCableRead(), []);
    await backend.triggerRequest(PD_5V, false);
    await sends(backend, () => backend.triggerCableRead(), [READ_LINE]);
  });

  it("AC-AXHK2P.2: out of step it is refused and sends nothing", async () => {
    const backend = await inPd("trigger-out-of-sync");
    await expect(backend.triggerCableRead()).rejects.toMatchObject({ kind: "out_of_sync" });
    await sends(backend, () => backend.triggerCableRead(), []);
  });

  it("AC-AXHK2P.4: `trigger-no-cable` answers no_answer after one send", async () => {
    expect(mockModeFromUrl("?mock=trigger-no-cable")).toBe("trigger-no-cable");
    const backend = await inPd("trigger-no-cable");
    await expect(backend.triggerCableRead()).resolves.toEqual({ kind: "no_answer" });
    await sends(backend, () => backend.triggerCableRead(), [READ_LINE]);
  });

  it("AC-AXHK2P.5: `trigger-unlisted-cable` answers a vendor outside the table", async () => {
    expect(mockModeFromUrl("?mock=trigger-unlisted-cable")).toBe("trigger-unlisted-cable");
    const backend = await inPd("trigger-unlisted-cable");
    await expect(backend.triggerCableRead()).resolves.toMatchObject({
      kind: "found",
      cable: { vid: 0x1234, vendor: null },
    });
  });

  it("a read ends the armed state, like any built message", async () => {
    const backend = await inPd();
    await backend.triggerRequest(PD_9V, true, true);
    await expect(backend.triggerArmed()).resolves.toBe(true);
    await backend.triggerCableRead();
    await expect(backend.triggerArmed()).resolves.toBe(false);
  });

  it("AC-AXHK2P.3: the built message needs no dialog", async () => {
    const backend = await inPd();
    await expect(backend.triggerMessageRisk(READ)).resolves.toBe("at_most_5v");
    await expect(backend.triggerMessageSend(READ, false)).resolves.toMatchObject({
      command: READ_LINE,
    });
  });

  it("AC-AXHK2P.3, .9: any other message to a cable keeps asking", async () => {
    const backend = await inPd();
    const others: PdMessageRequest[] = [
      { ...READ, sop: "sop_double_prime" },
      { ...READ, sop: "sop" },
      { ...READ, data_objects: [0xff00a802] },
      { ...READ, data_objects: [0xff00a841] },
      { ...READ, data_objects: [0xff008001] },
      { ...READ, data_objects: [0xff01a801] },
      { ...READ, data_objects: [0xff00a801, 0] },
    ];
    for (const message of others) {
      await expect(backend.triggerMessageRisk(message)).resolves.toBe("above_5v");
      await sends(backend, () => backend.triggerMessageSend(message, false), []);
    }
  });

  it("AC-AXHK2P.3: the raw console keeps asking for the same bytes", async () => {
    const backend = await inPd();
    await expect(backend.triggerConsoleRisk(READ_LINE)).resolves.toBe("above_5v");
    await sends(backend, () => backend.triggerConsoleSend(READ_LINE, false), []);
  });

  describe("AC-AXHK2P.6: the cable a capture holds", () => {
    it("with no capture there is none", async () => {
      const { backend } = await connected();
      await expect(backend.pdCableSeen()).resolves.toBeNull();
    });

    it("with no meter there is none, and no error", async () => {
      const backend = new MockBackend(null, { pdSynthetic: null });
      await expect(backend.pdCableSeen()).resolves.toBeNull();
    });

    it("a capture of the demo negotiation holds the sample cable, and nothing is sent", async () => {
      const { backend } = await connected();
      await backend.pdStart();
      await vi.advanceTimersByTimeAsync(5_000);
      await backend.pdStop();
      await expect(backend.pdCableSeen()).resolves.toMatchObject({ vid: 0x050d, pid: 0x8254 });
      expect(backend.triggerCommands()).toEqual([]);
    });

    it("a cleared capture holds none", async () => {
      const { backend } = await connected();
      await backend.pdStart();
      await vi.advanceTimersByTimeAsync(5_000);
      await backend.pdStop();
      await backend.pdClear();
      await expect(backend.pdCableSeen()).resolves.toBeNull();
    });
  });
});
