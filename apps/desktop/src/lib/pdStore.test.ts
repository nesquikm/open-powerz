/**
 * FR TGE83C (AC .2, .3, .6, .7): the PD analyzer's record store. Rows arrive from the
 * backend in batches (`pd_rows` events) and live in a column store; filters (GoodCRC
 * hidden by default, message type, SOP) pick store positions and never drop data.
 */

import { describe, expect, it } from "vitest";
import type { PdRow } from "./backend";
import { DEFAULT_PD_FILTER, PD_MAX_ROWS, PdStore, pdDroppedText, type PdFilter } from "./pdStore";

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

describe("a long capture is capped (AC-4PJRVK.3)", () => {
  const rows = (from: number, n: number, name = "Accept"): PdRow[] =>
    Array.from({ length: n }, (_, i) => msg(from + i, name));

  it("keeps every row up to the limit", () => {
    const store = new PdStore(4, { maxRows: 100 });
    store.append(rows(0, 100));
    expect(store.length).toBe(100);
    expect(store.dropped).toBe(0);
  });

  it("drops the oldest rows past the limit and counts them", () => {
    const store = new PdStore(4, { maxRows: 100 });
    store.append(rows(0, 60));
    store.append(rows(60, 90));
    expect(store.length).toBe(100);
    expect(store.dropped).toBe(50);
    expect(store.row(0).index).toBe(50);
    expect(store.row(99).index).toBe(149);
  });

  it("takes a single batch bigger than the limit", () => {
    const store = new PdStore(4, { maxRows: 10 });
    store.append(rows(0, 35));
    expect(store.length).toBe(10);
    expect(store.dropped).toBe(25);
    expect(store.row(0).index).toBe(25);
    expect(store.row(9).index).toBe(34);
  });

  it("finds the kept rows by index and not the dropped ones", () => {
    const store = new PdStore(4, { maxRows: 100 });
    store.append(rows(0, 150));
    expect(store.positionOf(49)).toBe(-1);
    expect(store.positionOf(50)).toBe(0);
    expect(store.positionOf(149)).toBe(99);
  });

  it("filters the kept rows only", () => {
    const store = new PdStore(4, { maxRows: 10 });
    store.append([msg(0, "Request"), ...rows(1, 20)]);
    const shown = store.filter({ ...DEFAULT_PD_FILTER, types: new Set(["Accept"]) });
    expect(shown.length).toBe(10);
    expect(store.filter({ ...DEFAULT_PD_FILTER, types: new Set(["Request"]) }).length).toBe(0);
  });

  it("keeps the session order flag true for sorted rows across the drop", () => {
    const store = new PdStore(4, { maxRows: 3 });
    store.append(rows(0, 8).map((r) => ({ ...r, session_ms: r.index * 10 })));
    expect(store.sessionSorted).toBe(true);
    expect(store.sessionMs(0)).toBe(50);
  });

  it("a clear starts the count again", () => {
    const store = new PdStore(4, { maxRows: 10 });
    store.append(rows(0, 15));
    store.clear();
    expect(store.dropped).toBe(0);
    expect(store.length).toBe(0);
    store.append(rows(15, 2));
    expect(store.row(0).index).toBe(15);
  });

  it("caps a default store too, above the 100,000 records the analyzer promises", () => {
    expect(PD_MAX_ROWS).toBeGreaterThanOrEqual(100_000);
    expect(Number.isFinite(PD_MAX_ROWS)).toBe(true);
    const store = new PdStore();
    store.append(rows(0, PD_MAX_ROWS + 5));
    expect(store.length).toBe(PD_MAX_ROWS);
    expect(store.dropped).toBe(5);
  });

  it("words the notice: nothing when nothing was dropped, else how many", () => {
    expect(pdDroppedText(0)).toBeNull();
    expect(pdDroppedText(1)).toContain("oldest");
    expect(pdDroppedText(1)).toContain("1 record");
    expect(pdDroppedText(1234)).toContain("1,234 records");
  });
});

describe("the type and SOP lists update incrementally (AC-4PJRVK.3)", () => {
  it("does not rebuild a list while nothing new arrived", () => {
    const store = new PdStore();
    store.append(negotiation());
    const types = store.messageTypes();
    const sops = store.sops();
    store.append([msg(100, "Accept")]);
    expect(store.messageTypes()).toBe(types);
    expect(store.sops()).toBe(sops);
  });

  it("gives a new list when a new type or SOP arrives, in first-seen order", () => {
    const store = new PdStore();
    store.append([msg(0, "Accept"), msg(1, "Request")]);
    const before = store.messageTypes();
    store.append([msg(2, "PS_RDY", "SOP'")]);
    expect(store.messageTypes()).not.toBe(before);
    expect(store.messageTypes()).toEqual(["Accept", "Request", "PS_RDY"]);
    expect(store.sops()).toEqual(["SOP", "SOP'"]);
  });

  it("does not list events", () => {
    const store = new PdStore();
    store.append([event(0, "Attach"), msg(1, "Accept")]);
    expect(store.messageTypes()).toEqual(["Accept"]);
  });

  it("starts over on a clear", () => {
    const store = new PdStore();
    store.append([msg(0, "Request", "SOP'"), msg(1, "Accept")]);
    expect(store.messageTypes()).toEqual(["Request", "Accept"]);
    store.clear();
    expect(store.messageTypes()).toEqual([]);
    expect(store.sops()).toEqual([]);
  });
});
