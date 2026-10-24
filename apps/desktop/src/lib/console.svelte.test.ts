/**
 * FR DGSQ6T (AC .1–.5): the PD message builder and the raw console's state. Built
 * messages and raw lines go through the backend's classifier; GoodCRC is refused with
 * a reason before anything reaches the backend; anything above 5 V (any unparsable
 * line too) waits for the trigger panel's confirmation and Cancel sends nothing; the
 * transcript keeps sent and received lines with their times; the cable preset's reply
 * shows the e-marker details.
 */

import { describe, expect, it } from "vitest";
import type { CableView, ConsoleLine, PdMessageRequest, PdSendReply, RiskClass } from "./backend";
import {
  DISCOVER_IDENTITY_CABLE,
  GOODCRC_REASON,
  PdConsole,
  formatLine,
  formatTime,
  lineRefusal,
  messageRefusal,
  messageTypeError,
  parseDataObjects,
  type ConsoleApi,
} from "./console.svelte";

const T = new Date(2026, 0, 1, 13, 4, 5, 67).getTime();

const CABLE: CableView = {
  vid: 0x050d,
  pid: 0x8254,
  active: false,
  max_voltage_v: 50,
  max_current_ma: 5000,
  epr_capable: true,
};

const REQUEST_9V: PdMessageRequest = { sop: "sop", msg_type: 2, data_objects: [0x2304b12c] };
const SOFT_RESET: PdMessageRequest = { sop: "sop", msg_type: 13, data_objects: [] };

/** A scripted backend: `pdm parm` answers, unknown text and `pd req=2` / Request 9 V
 * are above 5 V; records every send. */
class FakeApi implements ConsoleApi {
  sentLines: { line: string; confirmed: boolean }[] = [];
  sentMessages: { message: PdMessageRequest; confirmed: boolean }[] = [];
  riskCalls = 0;

  async messageRisk(message: PdMessageRequest): Promise<RiskClass> {
    this.riskCalls++;
    return message.msg_type === 2 ? "above_5v" : "at_most_5v";
  }
  async messageSend(message: PdMessageRequest, confirmed: boolean): Promise<PdSendReply> {
    this.sentMessages.push({ message, confirmed });
    const cable = message.sop === "sop_prime" && message.msg_type === 15 ? CABLE : null;
    return {
      command: "pd data=…",
      lines: [{ at_ms: T, direction: "tx", text: "pd data=…", block: null }],
      cable,
    };
  }
  async consoleRisk(line: string): Promise<RiskClass> {
    this.riskCalls++;
    if (line === "pdm parm") return "read_only";
    if (line === "entry pd") return "at_most_5v";
    return "above_5v";
  }
  async consoleSend(line: string, confirmed: boolean): Promise<ConsoleLine[]> {
    this.sentLines.push({ line, confirmed });
    const out: ConsoleLine[] = [{ at_ms: T, direction: "tx", text: line, block: null }];
    if (line === "pdm parm") {
      out.push({ at_ms: T + 12, direction: "rx", text: "type=0,em=2,sink=0", block: null });
    }
    return out;
  }
}

describe("AC-DGSQ6T.1: the builder's data objects", () => {
  it("parses hex words with or without 0x, space or comma separated", () => {
    expect(parseDataObjects("")).toEqual([]);
    expect(parseDataObjects("2304B12C")).toEqual([0x2304b12c]);
    expect(parseDataObjects("0xFF00A801, 1  ff")).toEqual([0xff00a801, 1, 0xff]);
  });

  it("refuses words that are not 32-bit hex, and more than 7 objects", () => {
    expect(parseDataObjects("xyz")).toBeNull();
    expect(parseDataObjects("123456789")).toBeNull();
    expect(parseDataObjects("1 2 3 4 5 6 7 8")).toBeNull();
    expect(parseDataObjects("1 2 3 4 5 6 7")).toHaveLength(7);
  });

  it("sends a control message and a data message through the backend", async () => {
    const api = new FakeApi();
    const c = new PdConsole(api);
    await c.sendMessage(SOFT_RESET);
    expect(api.sentMessages).toEqual([{ message: SOFT_RESET, confirmed: false }]);
    expect(c.lines.map((l) => l.text)).toContain("pd data=…");
  });
});

describe("AC-DGSQ6T.2: the Discover Identity preset", () => {
  it("is a structured VDM REQ for Discover Identity to SOP'", () => {
    expect(DISCOVER_IDENTITY_CABLE.sop).toBe("sop_prime");
    expect(DISCOVER_IDENTITY_CABLE.msg_type).toBe(15);
    expect(DISCOVER_IDENTITY_CABLE.data_objects).toHaveLength(1);
    const vdm = DISCOVER_IDENTITY_CABLE.data_objects[0]!;
    expect(vdm >>> 16).toBe(0xff00); // SVID: the PD SID
    expect((vdm >>> 15) & 1).toBe(1); // structured
    expect((vdm >>> 6) & 3).toBe(0); // REQ
    expect(vdm & 0x1f).toBe(1); // Discover Identity
  });

  it("keeps the cable's e-marker details from the reply", async () => {
    const c = new PdConsole(new FakeApi());
    await c.sendMessage(DISCOVER_IDENTITY_CABLE);
    expect(c.cable).toEqual(CABLE);
  });
});

describe("AC-DGSQ6T.3: GoodCRC is refused with a reason", () => {
  it("names GoodCRC in the reason", () => {
    expect(GOODCRC_REASON).toMatch(/GoodCRC/);
  });

  it("refuses a control GoodCRC in the builder on every SOP, not data type 1", () => {
    for (const sop of ["sop", "sop_prime", "sop_double_prime"] as const) {
      expect(messageRefusal({ sop, msg_type: 1, data_objects: [] })).toMatch(/GoodCRC/);
    }
    expect(messageRefusal({ sop: "sop", msg_type: 1, data_objects: [0x0001912c] })).toBeNull();
    expect(messageRefusal(SOFT_RESET)).toBeNull();
  });

  it("refuses pd cmd=1 and a pd data GoodCRC in the console", () => {
    expect(lineRefusal("pd cmd=1")).toMatch(/GoodCRC/);
    expect(lineRefusal("  pd cmd=1  ")).toMatch(/GoodCRC/);
    expect(lineRefusal("pd data=014100")).toMatch(/GoodCRC/);
    expect(lineRefusal("pd cmd=13")).toBeNull();
    expect(lineRefusal("pd cmd=10")).toBeNull();
    expect(lineRefusal("pdm parm")).toBeNull();
  });

  it("refuses GoodCRC in any spelling, like the backend", () => {
    for (const line of [
      "pd cmd=01",
      "pd cmd=+1",
      "pd cmd=0x1",
      "pd cmd=0X01",
      "pd  cmd=1",
      "PD CMD=1",
      "pd cmd = 1",
      "pd cmd=33",
      "PD DATA=014100",
      "pd  data=01 41 00",
    ]) {
      expect(lineRefusal(line), line).toMatch(/GoodCRC/);
    }
  });

  it("refuses pd cmd / pd data lines whose value cannot be read", () => {
    for (const line of [
      "pd cmd=",
      "pd cmd=-1",
      "pd cmd=1a",
      "pd cmdx=13",
      "pd cmd=99999999999999999999999",
      "pd data=zz",
      "pd data=014",
      "pd data=+14100",
      "pd data=0141",
      "pd data",
    ]) {
      expect(lineRefusal(line), line).not.toBeNull();
    }
    for (const line of ["pd cmd=013", "pd cmd=0xd", "pd data=008d00", "hello meter"]) {
      expect(lineRefusal(line), line).toBeNull();
    }
  });

  it("refuses pdm open and pdm close: the console owns the session", () => {
    for (const line of ["pdm close", "pdm open", "PDM  Close", "pdm closenow"]) {
      expect(lineRefusal(line), line).toMatch(/pdm close/);
    }
  });

  it("never reaches the backend, confirmed or not", async () => {
    const api = new FakeApi();
    const c = new PdConsole(api);
    await c.sendLine("pd cmd=1");
    expect(c.error?.message).toMatch(/GoodCRC/);
    await c.sendMessage({ sop: "sop_prime", msg_type: 1, data_objects: [] });
    expect(c.error?.message).toMatch(/GoodCRC/);
    expect(c.pending).toBeNull();
    expect(api.sentLines).toEqual([]);
    expect(api.sentMessages).toEqual([]);
  });
});

describe("AC-DGSQ6T.4: the console's transcript", () => {
  it("formats times as HH:MM:SS.mmm", () => {
    expect(formatTime(T)).toBe("13:04:05.067");
  });

  it("shows sent lines with > and received ones with <", () => {
    expect(formatLine({ at_ms: T, direction: "tx", text: "pdm parm", block: null })).toBe(
      "13:04:05.067 > pdm parm",
    );
    expect(
      formatLine({ at_ms: T + 1, direction: "rx", text: "type=0,em=2,sink=0", block: null }),
    ).toBe("13:04:05.068 < type=0,em=2,sink=0");
  });

  it("shows a parsed block instead of its tag", () => {
    const line: ConsoleLine = {
      at_ms: T,
      direction: "rx",
      text: "pdo:",
      block: "pdo: #1 Fixed 5.00 V 3.00 A, #2 Fixed 9.00 V 3.00 A",
    };
    expect(formatLine(line)).toBe(
      "13:04:05.067 < pdo: #1 Fixed 5.00 V 3.00 A, #2 Fixed 9.00 V 3.00 A",
    );
  });

  it("sends any line and appends both sides, oldest first", async () => {
    const api = new FakeApi();
    const c = new PdConsole(api);
    await c.sendLine("pdm parm");
    await c.sendLine("entry pd");
    expect(api.sentLines).toEqual([
      { line: "pdm parm", confirmed: false },
      { line: "entry pd", confirmed: false },
    ]);
    expect(c.lines.map((l) => [l.direction, l.text])).toEqual([
      ["tx", "pdm parm"],
      ["rx", "type=0,em=2,sink=0"],
      ["tx", "entry pd"],
    ]);
    c.clear();
    expect(c.lines).toEqual([]);
  });

  it("does not send an empty line", async () => {
    const api = new FakeApi();
    const c = new PdConsole(api);
    await c.sendLine("   ");
    expect(api.sentLines).toEqual([]);
    expect(api.riskCalls).toBe(0);
  });
});

describe("AC-DGSQ6T.5: above 5 V waits for the trigger panel's confirmation", () => {
  it("an unparsable line waits; Cancel sends nothing; Confirm sends it confirmed", async () => {
    const api = new FakeApi();
    const c = new PdConsole(api);
    await c.sendLine("hello meter");
    expect(api.sentLines).toEqual([]);
    expect(c.pending?.kind).toBe("line");
    expect(c.pending?.line).toBe("hello meter");
    expect(c.pending?.text).toMatch(/Confirm that nothing unsafe is connected to the output/);
    c.cancel();
    expect(c.pending).toBeNull();
    expect(api.sentLines).toEqual([]);
    await c.sendLine("hello meter");
    await c.confirm();
    expect(api.sentLines).toEqual([{ line: "hello meter", confirmed: true }]);
    expect(c.pending).toBeNull();
  });

  it("a built Request for a higher voltage waits for the confirmation", async () => {
    const api = new FakeApi();
    const c = new PdConsole(api);
    await c.sendMessage(REQUEST_9V);
    expect(api.sentMessages).toEqual([]);
    expect(c.pending?.kind).toBe("message");
    expect(c.pending?.message).toEqual(REQUEST_9V);
    expect(c.pending?.text).toMatch(/Confirm that nothing unsafe is connected to the output/);
    await c.confirm();
    expect(api.sentMessages).toEqual([{ message: REQUEST_9V, confirmed: true }]);
  });

  it("at or below 5 V nothing is asked", async () => {
    const api = new FakeApi();
    const c = new PdConsole(api);
    await c.sendLine("entry pd");
    expect(c.pending).toBeNull();
    expect(api.sentLines).toEqual([{ line: "entry pd", confirmed: false }]);
  });

  it("a backend refusal of a confirmed line is shown and frees the controls", async () => {
    const api = new FakeApi();
    api.consoleSend = async () => {
      throw { kind: "internal", message: "Talking to the meter's serial port failed.", hint: null };
    };
    const c = new PdConsole(api);
    await c.sendLine("hello meter");
    await c.confirm();
    expect(c.error?.message).toMatch(/serial port/);
    expect(c.busy).toBe(false);
  });

  it("a backend refusal is shown as the error", async () => {
    const api = new FakeApi();
    api.consoleSend = async () => {
      throw { kind: "usage", message: "A request above 5 V needs your confirmation.", hint: null };
    };
    const c = new PdConsole(api);
    await c.sendLine("entry pd");
    expect(c.error?.message).toMatch(/5 V/);
  });
});

/** A promise and its resolver, for holding a backend call in flight. */
function deferred<T>(): { promise: Promise<T>; resolve: (v: T) => void } {
  let resolve!: (v: T) => void;
  const promise = new Promise<T>((r) => (resolve = r));
  return { promise, resolve };
}

describe("one send at a time", () => {
  it("is busy while a send is in flight and ignores further sends", async () => {
    const api = new FakeApi();
    const held = deferred<ConsoleLine[]>();
    api.consoleSend = async (line, confirmed) => {
      api.sentLines.push({ line, confirmed });
      return held.promise;
    };
    const c = new PdConsole(api);
    const first = c.sendLine("pdm parm");
    await Promise.resolve();
    await Promise.resolve();
    expect(c.busy).toBe(true);
    await c.sendLine("entry pd");
    await c.sendMessage(SOFT_RESET);
    expect(api.sentLines).toEqual([{ line: "pdm parm", confirmed: false }]);
    expect(api.sentMessages).toEqual([]);
    held.resolve([{ at_ms: T, direction: "tx", text: "pdm parm", block: null }]);
    await first;
    expect(c.busy).toBe(false);
    expect(c.lines.map((l) => l.text)).toEqual(["pdm parm"]);
  });

  it("appends to the transcript as it is when the reply arrives", async () => {
    const api = new FakeApi();
    const held = deferred<ConsoleLine[]>();
    api.consoleSend = async () => held.promise;
    const c = new PdConsole(api);
    c.lines = [{ at_ms: T, direction: "tx", text: "old", block: null }];
    const send = c.sendLine("pdm parm");
    await Promise.resolve();
    await Promise.resolve();
    // The transcript changes while the send is in flight.
    c.clear();
    c.lines = [{ at_ms: T + 1, direction: "rx", text: "meanwhile", block: null }];
    held.resolve([{ at_ms: T + 2, direction: "tx", text: "pdm parm", block: null }]);
    await send;
    expect(c.lines.map((l) => l.text)).toEqual(["meanwhile", "pdm parm"]);
  });

  it("confirm is ignored while a send is in flight", async () => {
    const api = new FakeApi();
    const c = new PdConsole(api);
    await c.sendLine("hello meter");
    expect(c.pending).not.toBeNull();
    c.busy = true;
    await c.confirm();
    expect(api.sentLines).toEqual([]);
    expect(c.pending).not.toBeNull();
  });
});

describe("the builder's message type", () => {
  it("is a whole number from 0 to 31", () => {
    expect(messageTypeError(0)).toBeNull();
    expect(messageTypeError(13)).toBeNull();
    expect(messageTypeError(31)).toBeNull();
    for (const bad of [-1, 32, 1.5, NaN, null, undefined]) {
      expect(messageTypeError(bad), String(bad)).toMatch(/0 to 31/);
    }
  });
});

describe("a closed trigger session", () => {
  it("reset empties the transcript, cable, error and pending send", async () => {
    const c = new PdConsole(new FakeApi());
    await c.sendMessage(DISCOVER_IDENTITY_CABLE);
    await c.sendLine("pdm parm");
    await c.sendLine("hello meter");
    c.error = { kind: "usage", message: "x", hint: null };
    expect(c.lines).not.toEqual([]);
    expect(c.cable).not.toBeNull();
    expect(c.pending).not.toBeNull();
    c.reset();
    expect(c.lines).toEqual([]);
    expect(c.cable).toBeNull();
    expect(c.error).toBeNull();
    expect(c.pending).toBeNull();
  });
});
