/**
 * FR TGE83C (AC .2, .3, .6, .7): the PD analyzer's record store. Rows arrive from the
 * backend in batches (`pd_rows` events) and live in a column store; filters (GoodCRC
 * hidden by default, message type, SOP) pick store positions and never drop data.
 */

import { describe, expect, it } from "vitest";
import type { PdRow } from "./backend";
import { DEFAULT_PD_FILTER, PdStore, type PdFilter } from "./pdStore";

function msg(index: number, name: string, sop = "SOP", extra: Partial<PdRow> = {}): PdRow {
  return {
    index,
    elapsed_ms: index * 10,
    kind: "message",
    sop,
    name,
    message_id: index % 8,
    direction: "source_to_sink",
    objects: 0,
    spec_revision: "3.x",
    vbus_v: 5,
    ibus_a: 0.5,
    summary: name,
    ...extra,
  };
}

function event(index: number, name: string): PdRow {
  return {
    index,
    elapsed_ms: index * 10,
    kind: "event",
    sop: null,
    name,
    message_id: null,
    direction: null,
    objects: null,
    spec_revision: null,
    vbus_v: 0,
    ibus_a: 0,
    summary: name,
  };
}

/** Attach, then a negotiation with a GoodCRC after every message, plus a cable message. */
function negotiation(): PdRow[] {
  return [
    event(0, "Attach"),
    msg(1, "Source_Capabilities", "SOP", {
      objects: 4,
      summary: "Source_Capabilities: Fixed 5.00 V 3.00 A",
    }),
    msg(2, "GoodCRC"),
    msg(3, "Request", "SOP", { direction: "sink_to_source", objects: 1 }),
    msg(4, "GoodCRC", "SOP", { direction: "source_to_sink" }),
    msg(5, "Accept"),
    msg(6, "GoodCRC", "SOP", { direction: "sink_to_source" }),
    msg(7, "Vendor_Defined", "SOP'", { direction: "cable", objects: 1 }),
    msg(8, "GoodCRC", "SOP'", { direction: "cable" }),
    msg(9, "PS_RDY"),
    event(10, "Hard Reset"),
  ];
}

/** The names of the rows at `positions`. */
function names(store: PdStore, positions: ArrayLike<number>): string[] {
  return Array.from(positions, (p) => store.row(p).name);
}

const ALL: PdFilter = { showGoodCrc: true, types: null, sop: null };

describe("PdStore column store (AC-TGE83C.2)", () => {
  it("keeps every appended row, in order, and reads each back whole", () => {
    const store = new PdStore();
    const rows = negotiation();
    store.append(rows.slice(0, 4));
    store.append(rows.slice(4));
    expect(store.length).toBe(rows.length);
    for (let i = 0; i < rows.length; i++) expect(store.row(i)).toEqual(rows[i]);
  });

  it("grows past its first capacity without losing rows", () => {
    const store = new PdStore(4);
    const rows = Array.from({ length: 5000 }, (_, i) => msg(i, i % 2 ? "GoodCRC" : "Request"));
    for (let i = 0; i < rows.length; i += 37) store.append(rows.slice(i, i + 37));
    expect(store.length).toBe(5000);
    expect(store.row(4999)).toEqual(rows[4999]);
    expect(store.row(0)).toEqual(rows[0]);
  });

  it("clear empties it, and the next rows start over", () => {
    const store = new PdStore();
    store.append(negotiation());
    store.clear();
    expect(store.length).toBe(0);
    expect(store.filter(ALL)).toHaveLength(0);
    store.append([msg(0, "Accept")]);
    expect(store.length).toBe(1);
    expect(store.row(0).name).toBe("Accept");
  });

  it("lists the message types and SOPs seen, for the filter menus", () => {
    const store = new PdStore();
    store.append(negotiation());
    expect(store.messageTypes()).toEqual(
      expect.arrayContaining(["Source_Capabilities", "Request", "Accept", "GoodCRC", "PS_RDY"]),
    );
    expect(new Set(store.messageTypes()).size).toBe(store.messageTypes().length);
    expect([...store.sops()].sort()).toEqual(["SOP", "SOP'"]);
  });
});

describe("PdStore filters (AC-TGE83C.3)", () => {
  it("hides GoodCRC by default", () => {
    expect(DEFAULT_PD_FILTER.showGoodCrc).toBe(false);
    expect(DEFAULT_PD_FILTER.types).toBeNull();
    expect(DEFAULT_PD_FILTER.sop).toBeNull();
    const store = new PdStore();
    store.append(negotiation());
    const shown = names(store, store.filter(DEFAULT_PD_FILTER));
    expect(shown).not.toContain("GoodCRC");
    expect(shown).toEqual([
      "Attach",
      "Source_Capabilities",
      "Request",
      "Accept",
      "Vendor_Defined",
      "PS_RDY",
      "Hard Reset",
    ]);
  });

  it("the GoodCRC toggle shows them again, in capture order", () => {
    const store = new PdStore();
    store.append(negotiation());
    const shown = store.filter({ ...DEFAULT_PD_FILTER, showGoodCrc: true });
    expect(Array.from(shown)).toEqual(negotiation().map((_, i) => i));
  });

  it("a message type filter keeps only those types", () => {
    const store = new PdStore();
    store.append(negotiation());
    const shown = store.filter({ ...ALL, types: new Set(["Request", "Accept"]) });
    expect(names(store, shown)).toEqual(["Request", "Accept"]);
  });

  it("a SOP filter narrows the messages; events stay", () => {
    const store = new PdStore();
    store.append(negotiation());
    const shown = names(store, store.filter({ ...ALL, sop: "SOP'" }));
    expect(shown).toEqual(["Attach", "Vendor_Defined", "GoodCRC", "Hard Reset"]);
  });

  it("filters combine, and none of them drops captured data", () => {
    const store = new PdStore();
    store.append(negotiation());
    const narrow = store.filter({
      showGoodCrc: false,
      types: new Set(["Vendor_Defined"]),
      sop: "SOP'",
    });
    expect(names(store, narrow)).toEqual(["Vendor_Defined"]);
    expect(store.length).toBe(negotiation().length);
    expect(store.filter(ALL)).toHaveLength(negotiation().length);
  });

  it("filters 100,000 rows quickly (AC-TGE83C.6)", () => {
    const store = new PdStore();
    const types = ["Source_Capabilities", "GoodCRC", "Request", "GoodCRC", "Accept", "GoodCRC"];
    const rows = Array.from({ length: 100_000 }, (_, i) => msg(i, types[i % types.length]!));
    for (let i = 0; i < rows.length; i += 500) store.append(rows.slice(i, i + 500));
    const t0 = performance.now();
    const shown = store.filter(DEFAULT_PD_FILTER);
    const typed = store.filter({ ...DEFAULT_PD_FILTER, types: new Set(["Request"]) });
    const took = performance.now() - t0;
    expect(shown).toHaveLength(50_000);
    expect(typed).toHaveLength(Math.ceil(100_000 / 6));
    expect(took).toBeLessThan(50);
  });
});
