/**
 * FR DGSQ6T: the mock backend's PD message builder and raw console, which the
 * Playwright suite drives. Like the backend it renders built messages as `pd cmd=` /
 * `pd data=`, answers the Discover Identity preset with the demo e-marked cable
 * (VID 0x050D, PID 0x8254, 50 V, 5 A, EPR), refuses GoodCRC with a reason, answers
 * `pdm parm`, classifies with the 5 V risk gate (unknown text is above 5 V) and
 * refuses an unconfirmed above-5 V send without sending anything.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { PdMessageRequest } from "./backend";
import { MockBackend } from "./mockBackend";

const DISCOVER_CABLE: PdMessageRequest = {
  sop: "sop_prime",
  msg_type: 15,
  data_objects: [0xff00a801],
};
const REQUEST_9V: PdMessageRequest = { sop: "sop", msg_type: 2, data_objects: [0x2304b12c] };

async function opened(): Promise<MockBackend> {
  const backend = new MockBackend(null, { pdSynthetic: null });
  await backend.connect("demo:load-5v-1a", 50, () => {});
  await backend.triggerOpen();
  return backend;
}

beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());

describe("AC-DGSQ6T.1: built messages", () => {
  it("needs an open trigger session", async () => {
    const backend = new MockBackend(null, { pdSynthetic: null });
    await expect(
      backend.triggerMessageSend({ sop: "sop", msg_type: 13, data_objects: [] }, false),
    ).rejects.toMatchObject({ kind: "usage" });
    await expect(backend.triggerConsoleSend("pdm parm", false)).rejects.toMatchObject({
      kind: "usage",
    });
  });

  it("renders control messages as pd cmd on SOP and pd data to the cable", async () => {
    const backend = await opened();
    const a = await backend.triggerMessageSend(
      { sop: "sop", msg_type: 13, data_objects: [] },
      false,
    );
    expect(a.command).toBe("pd cmd=13");
    const b = await backend.triggerMessageSend(
      { sop: "sop_double_prime", msg_type: 13, data_objects: [] },
      false,
    );
    expect(b.command).toBe("pd data=028D00");
    expect(backend.triggerCommands()).toEqual(
      expect.arrayContaining(["pd cmd=13", "pd data=028D00"]),
    );
    expect(b.lines[0]).toMatchObject({ direction: "tx", text: "pd data=028D00" });
  });
});

describe("AC-DGSQ6T.2: the cable answers Discover Identity", () => {
  it("returns the demo cable's e-marker details", async () => {
    const backend = await opened();
    const r = await backend.triggerMessageSend(DISCOVER_CABLE, true);
    expect(r.command.startsWith("pd data=01")).toBe(true);
    expect(r.cable).toEqual({
      vid: 0x050d,
      pid: 0x8254,
      active: false,
      max_voltage_v: 50,
      max_current_ma: 5000,
      epr_capable: true,
    });
  });
});

describe("AC-DGSQ6T.3: GoodCRC is refused", () => {
  it("in the builder and the console, with a reason, sending nothing", async () => {
    const backend = await opened();
    const before = backend.triggerCommands().length;
    await expect(
      backend.triggerMessageSend({ sop: "sop", msg_type: 1, data_objects: [] }, true),
    ).rejects.toMatchObject({ kind: "usage", message: expect.stringMatching(/GoodCRC/) });
    await expect(
      backend.triggerMessageRisk({ sop: "sop_prime", msg_type: 1, data_objects: [] }),
    ).rejects.toMatchObject({ message: expect.stringMatching(/GoodCRC/) });
    await expect(backend.triggerConsoleSend("pd cmd=1", true)).rejects.toMatchObject({
      kind: "usage",
      message: expect.stringMatching(/GoodCRC/),
    });
    expect(backend.triggerCommands()).toHaveLength(before);
  });
});

describe("AC-DGSQ6T.4: the console", () => {
  it("answers pdm parm with timed sent and received lines", async () => {
    const backend = await opened();
    const lines = await backend.triggerConsoleSend("pdm parm", false);
    expect(lines.map((l) => [l.direction, l.text])).toEqual([
      ["tx", "pdm parm"],
      ["rx", "type=0,em=2,sink=0"],
    ]);
    expect(lines[0]!.at_ms).toBeGreaterThan(0);
    expect(lines[1]!.at_ms).toBeGreaterThanOrEqual(lines[0]!.at_ms);
    expect(backend.triggerCommands()).toContain("pdm parm");
  });

  it("shows the pdo block parsed after entry pd", async () => {
    const backend = await opened();
    const lines = await backend.triggerConsoleSend("entry pd", false);
    const block = lines.find((l) => l.block !== null)?.block ?? "";
    expect(block).toMatch(/9\.00 V/);
  });
});

describe("AC-DGSQ6T.5: above 5 V needs the confirmation", () => {
  it("classifies unknown text and pd req=2 above 5 V, pdm parm read-only", async () => {
    const backend = await opened();
    expect(await backend.triggerConsoleRisk("hello meter")).toBe("above_5v");
    expect(await backend.triggerConsoleRisk("pd req=2")).toBe("above_5v");
    expect(await backend.triggerConsoleRisk("pdm parm")).toBe("read_only");
    expect(await backend.triggerMessageRisk(REQUEST_9V)).toBe("above_5v");
    expect(await backend.triggerMessageRisk({ sop: "sop", msg_type: 13, data_objects: [] })).toBe(
      "at_most_5v",
    );
  });

  it("refuses an unconfirmed above-5 V send and sends it once confirmed", async () => {
    const backend = await opened();
    const before = backend.triggerCommands().length;
    await expect(backend.triggerConsoleSend("hello meter", false)).rejects.toMatchObject({
      kind: "usage",
    });
    await expect(backend.triggerMessageSend(REQUEST_9V, false)).rejects.toMatchObject({
      kind: "usage",
    });
    expect(backend.triggerCommands()).toHaveLength(before);
    await backend.triggerConsoleSend("hello meter", true);
    const r = await backend.triggerMessageSend(REQUEST_9V, true);
    expect(r.command).toBe("pd data=0082102CB10423");
    expect(backend.triggerCommands().slice(before)).toEqual([
      "hello meter",
      "pd data=0082102CB10423",
    ]);
  });
});
