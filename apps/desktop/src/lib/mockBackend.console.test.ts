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
      "entry pd",
      "pd data=0082102CB10423",
    ]);
  });
});

describe("AC-4PJRVH.8: the mock mirrors the shipped trigger rules", () => {
  const CONTROL_13: PdMessageRequest = { sop: "sop", msg_type: 13, data_objects: [] };

  describe("PD entry before built messages", () => {
    it("enters PD first when another protocol was entered last", async () => {
      const backend = await opened();
      await backend.triggerList("qc");
      const before = backend.triggerCommands().length;

      await backend.triggerMessageSend(CONTROL_13, false);

      expect(backend.triggerCommands().slice(before).at(0)).toBe("entry pd");
      expect(backend.triggerCommands().at(-1)).toBe("pd cmd=13");
    });

    it("enters PD first when nothing was entered yet", async () => {
      const backend = await opened();
      await backend.triggerMessageSend(CONTROL_13, false);
      expect(backend.triggerCommands().slice(1).at(0)).toBe("entry pd");
    });

    it("the Discover Identity preset enters PD first too", async () => {
      const backend = await opened();
      await backend.triggerList("qc");
      const before = backend.triggerCommands().length;

      await backend.triggerMessageSend(DISCOVER_CABLE, true);

      expect(backend.triggerCommands().slice(before).at(0)).toBe("entry pd");
    });

    it("does not enter again with PD entered last", async () => {
      const backend = await opened();
      await backend.triggerList("pd");
      const before = backend.triggerCommands().length;

      await backend.triggerMessageSend(CONTROL_13, false);

      expect(backend.triggerCommands().slice(before)).toEqual(["pd cmd=13"]);
    });

    it("a refused message enters nothing", async () => {
      const backend = await opened();
      await backend.triggerList("qc");
      const before = backend.triggerCommands().length;

      await expect(backend.triggerMessageSend(REQUEST_9V, false)).rejects.toMatchObject({
        kind: "usage",
      });

      expect(backend.triggerCommands().slice(before)).toEqual([]);
    });

    it("Back to 5 V after a built message goes through PD", async () => {
      const backend = await opened();
      await backend.triggerList("qc");
      await backend.triggerMessageSend(CONTROL_13, false);

      await backend.triggerBackTo5v();

      expect(backend.triggerCommands().at(-1)).toBe("pd req=1");
    });
  });

  describe("cable match by VDM", () => {
    it.each<[string, PdMessageRequest]>([
      ["SVDM version 1.0", { sop: "sop_prime", msg_type: 15, data_objects: [0xff008001] }],
      ["the far plug", { sop: "sop_double_prime", msg_type: 15, data_objects: [0xff00a801] }],
      ["extra objects", { sop: "sop_prime", msg_type: 15, data_objects: [0xff00a801, 0] }],
    ])("a Discover Identity REQ is answered: %s", async (_name, message) => {
      const backend = await opened();
      const reply = await backend.triggerMessageSend(message, true);
      expect(reply.cable?.vid).toBe(0x050d);
    });

    it.each<[string, PdMessageRequest]>([
      ["an ACK", { sop: "sop_prime", msg_type: 15, data_objects: [0xff00a841] }],
      ["Discover SVIDs", { sop: "sop_prime", msg_type: 15, data_objects: [0xff00a802] }],
      ["an unstructured VDM", { sop: "sop_prime", msg_type: 15, data_objects: [0xff002801] }],
      ["to the charger", { sop: "sop", msg_type: 15, data_objects: [0xff00a801] }],
    ])("anything else is not: %s", async (_name, message) => {
      const backend = await opened();
      const reply = await backend.triggerMessageSend(message, true);
      expect(reply.cable).toBeNull();
    });
  });

  describe("swaps above 5 V", () => {
    it.each([9, 10, 11, 19])("console pd cmd=%i needs a confirmation", async (ty) => {
      const backend = await opened();
      const before = backend.triggerCommands().length;

      expect(await backend.triggerConsoleRisk(`pd cmd=${ty}`)).toBe("above_5v");
      await expect(backend.triggerConsoleSend(`pd cmd=${ty}`, false)).rejects.toMatchObject({
        kind: "usage",
      });

      expect(backend.triggerCommands().slice(before)).toEqual([]);
    });

    it.each([9, 10, 11, 19])("a built swap of type %i needs a confirmation", async (ty) => {
      const backend = await opened();
      await backend.triggerList("pd");
      const before = backend.triggerCommands().length;

      for (const sop of ["sop", "sop_prime"] as const) {
        const swap: PdMessageRequest = { sop, msg_type: ty, data_objects: [] };
        expect(await backend.triggerMessageRisk(swap)).toBe("above_5v");
        await expect(backend.triggerMessageSend(swap, false)).rejects.toMatchObject({
          kind: "usage",
        });
      }

      expect(backend.triggerCommands().slice(before)).toEqual([]);
    });

    it("other control messages stay at most 5 V", async () => {
      const backend = await opened();
      expect(await backend.triggerConsoleRisk("pd cmd=13")).toBe("at_most_5v");
      expect(await backend.triggerMessageRisk(CONTROL_13)).toBe("at_most_5v");
    });
  });

  describe("reserved control types", () => {
    it.each([0, 0x19, 0x1f])("control type %i is refused as reserved", async (ty) => {
      const backend = await opened();
      await backend.triggerList("pd");
      const before = backend.triggerCommands().length;
      const message: PdMessageRequest = { sop: "sop", msg_type: ty, data_objects: [] };

      await expect(backend.triggerMessageRisk(message)).rejects.toMatchObject({
        kind: "usage",
        message: expect.stringMatching(/reserved/i),
      });
      await expect(backend.triggerMessageSend(message, true)).rejects.toMatchObject({
        kind: "usage",
        message: expect.stringMatching(/reserved/i),
      });

      expect(backend.triggerCommands().slice(before)).toEqual([]);
    });

    it("type 2 without objects is a data message missing its object, not reserved", async () => {
      const backend = await opened();
      await expect(
        backend.triggerMessageSend({ sop: "sop", msg_type: 2, data_objects: [] }, true),
      ).rejects.toMatchObject({
        kind: "usage",
        message: expect.stringMatching(/data object/i),
      });
    });
  });
});
