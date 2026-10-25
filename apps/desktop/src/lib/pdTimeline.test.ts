/**
 * FR TGE83D: the PD timeline on the power chart, free of the DOM.
 *
 * - Rows carry `session_ms`, their time on the chart's session time base; the store
 *   keeps it and finds a row's store position by record index (AC .1, .2).
 * - Marker kinds and their legend: negotiation, reset, error, attach and detach, other;
 *   one distinct colour per kind in each theme (AC .1).
 * - Markers closer than 4 px merge into one counted badge; zooming in splits them (AC .5).
 * - A selected row brings its time into the chart's view, keeping the zoom (AC .3).
 * - The PD status blocks (VBUS, IBUS, CC1, CC2) are kept for plotting (AC .4).
 */

import { describe, expect, it } from "vitest";
import type { BackendEvent, PdRow, PdStatusSample } from "./backend";
import { PdCapture } from "./pdCapture.svelte";
import { PdStore } from "./pdStore";
import {
  PD_MARKER_KINDS,
  PD_MARKER_MERGE_PX,
  PdStatusBuffer,
  clusterMarkers,
  hitMarker,
  markerColors,
  markerKind,
  markersInRange,
  revealTime,
  type PdMarker,
  type PdMarkerKind,
} from "./pdTimeline";

function row(
  index: number,
  name: string,
  sessionMs: number,
  kind: PdRow["kind"] = "message",
): PdRow {
  const isEvent = kind === "event";
  return {
    index,
    elapsed_ms: sessionMs - 1_000,
    session_ms: sessionMs,
    kind,
    sop: isEvent ? null : "SOP",
    name,
    message_id: isEvent ? null : index % 8,
    direction: isEvent ? null : "source_to_sink",
    objects: isEvent ? null : 0,
    spec_revision: isEvent ? null : "3.x",
    vbus_v: 5,
    ibus_a: 0.5,
    summary: name,
  };
}

const ev = (index: number, name: string, ms: number) => row(index, name, ms, "event");
const msg = (index: number, name: string, ms: number) => row(index, name, ms, "message");

/** A linear time → px mapping: `pxPerMs` px per ms from `t0`. */
function scale(t0: number, pxPerMs: number): (ms: number) => number {
  return (ms) => (ms - t0) * pxPerMs;
}

function marker(index: number, ms: number, kind: PdMarkerKind = "negotiation"): PdMarker {
  return { index, ms, kind };
}

// ---- AC-TGE83D.1: session time on rows; marker kinds, colours, legend ----

describe("PD rows on the session time base (AC-TGE83D.1)", () => {
  it("the store keeps each row's session time", () => {
    const store = new PdStore();
    store.append([ev(0, "Attach", 30_000), msg(1, "Source_Capabilities", 30_280)]);
    expect(store.row(0).session_ms).toBe(30_000);
    expect(store.row(1).session_ms).toBe(30_280);
    expect(store.row(1).elapsed_ms).toBe(29_280);
  });

  it("finds the markers inside a session time range, in capture order, with their kind", () => {
    const store = new PdStore();
    store.append([
      ev(0, "Attach", 1_000),
      msg(1, "Source_Capabilities", 1_280),
      msg(2, "Request", 1_290),
      ev(3, "Hard Reset", 1_700),
      msg(4, "Vendor_Defined", 2_250),
    ]);
    expect(markersInRange(store, 1_100, 1_700)).toEqual([
      { index: 1, ms: 1_280, kind: "negotiation" },
      { index: 2, ms: 1_290, kind: "negotiation" },
      { index: 3, ms: 1_700, kind: "reset" },
    ]);
    expect(markersInRange(store, 0, 999)).toEqual([]);
    expect(markersInRange(store, 0, 10_000).map((m) => m.index)).toEqual([0, 1, 2, 3, 4]);
  });
});

describe("marker kinds (AC-TGE83D.1)", () => {
  it.each([
    ["Source_Capabilities", "message"],
    ["Request", "message"],
    ["Accept", "message"],
    ["PS_RDY", "message"],
    ["EPR_Request", "message"],
    ["EPR_Source_Capabilities", "message"],
    ["EPR_Mode", "message"],
  ] as const)("%s is negotiation", (name, kind) => {
    expect(markerKind({ name, kind })).toBe("negotiation");
  });

  it.each([
    ["Hard Reset", "event"],
    ["Cable Reset", "event"],
    ["Soft_Reset", "message"],
  ] as const)("%s is a reset", (name, kind) => {
    expect(markerKind({ name, kind })).toBe("reset");
  });

  it.each([
    ["Receive Error (CrcCheck)", "event"],
    ["Bad Message ID", "event"],
  ] as const)("%s is an error", (name, kind) => {
    expect(markerKind({ name, kind })).toBe("error");
  });

  it.each([
    ["Attach", "event"],
    ["Detach", "event"],
  ] as const)("%s is attach and detach", (name, kind) => {
    expect(markerKind({ name, kind })).toBe("attach_detach");
  });

  it.each([
    ["GoodCRC", "message"],
    ["Vendor_Defined", "message"],
    ["Monitor Type (Pd)", "event"],
  ] as const)("%s is other", (name, kind) => {
    expect(markerKind({ name, kind })).toBe("other");
  });

  it("the legend names every kind once, in a fixed order", () => {
    expect(PD_MARKER_KINDS.map((k) => k.kind)).toEqual([
      "negotiation",
      "reset",
      "error",
      "attach_detach",
      "other",
    ]);
    expect(PD_MARKER_KINDS.map((k) => k.label)).toEqual([
      "Negotiation",
      "Reset",
      "Error",
      "Attach / detach",
      "Other",
    ]);
  });

  it.each(["light", "dark"] as const)("every kind has its own colour in the %s theme", (theme) => {
    const colors = markerColors(theme);
    const values = PD_MARKER_KINDS.map((k) => colors[k.kind]);
    for (const c of values) expect(c).toMatch(/^(#[0-9a-f]{3,8}|rgba?\(|hsla?\(|oklch\()/i);
    expect(new Set(values.map((c) => c.toLowerCase())).size).toBe(PD_MARKER_KINDS.length);
  });
});

// ---- AC-TGE83D.2: a marker click finds its row ----

describe("marker hits and row positions (AC-TGE83D.2)", () => {
  it("a click on a marker hits it, a click beside the markers hits nothing", () => {
    const toPx = scale(0, 1);
    const clusters = clusterMarkers([marker(4, 100), marker(9, 300)], toPx);
    expect(hitMarker(clusters, 101)?.markers.map((m) => m.index)).toEqual([4]);
    expect(hitMarker(clusters, 298)?.markers.map((m) => m.index)).toEqual([9]);
    expect(hitMarker(clusters, 200)).toBeNull();
  });

  it("the store finds a row's position by its record index, also after a clear", () => {
    const store = new PdStore();
    store.append([msg(0, "Accept", 10), msg(1, "Accept", 20), msg(2, "Accept", 30)]);
    expect(store.positionOf(2)).toBe(2);
    expect(store.positionOf(7)).toBe(-1);
    store.clear();
    // Indices keep counting across a clear.
    store.append([msg(40, "Accept", 400), msg(41, "Request", 410), msg(42, "Accept", 420)]);
    expect(store.positionOf(41)).toBe(1);
    expect(store.positionOf(1)).toBe(-1);
  });

  it("finds a position among 100,000 rows quickly", () => {
    const store = new PdStore();
    const rows = Array.from({ length: 100_000 }, (_, i) => msg(i, "Accept", i * 2));
    for (let i = 0; i < rows.length; i += 1_000) store.append(rows.slice(i, i + 1_000));
    const t0 = performance.now();
    let sum = 0;
    for (let k = 0; k < 1_000; k++) sum += store.positionOf((k * 97) % 100_000);
    const took = performance.now() - t0;
    expect(sum).toBeGreaterThan(0);
    expect(took).toBeLessThan(50);
  });
});

// ---- AC-TGE83D.3: a selected row comes into the chart's view ----

describe("revealTime (AC-TGE83D.3)", () => {
  const session = { firstMs: 0, lastMs: 100_000 };

  it("keeps the view, zoom and all, when the time is already visible", () => {
    const view = { t0Ms: 10_000, t1Ms: 20_000 };
    expect(revealTime(view, 15_000, session)).toBe(view);
    expect(revealTime(view, 10_000, session)).toBe(view);
    expect(revealTime(view, 20_000, session)).toBe(view);
  });

  it("centres a time before the view, keeping the span", () => {
    const view = { t0Ms: 50_000, t1Ms: 51_000 };
    expect(revealTime(view, 30_000, session)).toEqual({ t0Ms: 29_500, t1Ms: 30_500 });
  });

  it("centres a time after the view, keeping the span", () => {
    const view = { t0Ms: 10_000, t1Ms: 14_000 };
    expect(revealTime(view, 60_000, session)).toEqual({ t0Ms: 58_000, t1Ms: 62_000 });
  });

  it("stays inside the session near its ends", () => {
    const view = { t0Ms: 50_000, t1Ms: 60_000 };
    expect(revealTime(view, 1_000, session)).toEqual({ t0Ms: 0, t1Ms: 10_000 });
    expect(revealTime(view, 99_000, session)).toEqual({ t0Ms: 90_000, t1Ms: 100_000 });
  });
});

// ---- AC-TGE83D.4: PD status blocks for plotting ----

function status(ms: number, cc1 = 1.528): PdStatusSample {
  return { session_ms: ms, vbus_v: 5, ibus_a: -0.25, cc1_v: cc1, cc2_v: 0 };
}

describe("PD status samples (AC-TGE83D.4)", () => {
  it("the buffer returns VBUS, IBUS, CC1 and CC2 columns for a time range", () => {
    const buf = new PdStatusBuffer();
    buf.append([status(100, 0), status(105), status(110)]);
    buf.append([status(115), status(120)]);
    expect(buf.length).toBe(5);
    const r = buf.range(105, 115);
    expect(r.t).toEqual([105, 110, 115]);
    expect(r.vbus).toEqual([5, 5, 5]);
    expect(r.ibus).toEqual([-0.25, -0.25, -0.25]);
    expect(r.cc1).toEqual([1.528, 1.528, 1.528]);
    expect(r.cc2).toEqual([0, 0, 0]);
    expect(buf.range(0, 99).t).toEqual([]);
  });

  it("clear empties the buffer", () => {
    const buf = new PdStatusBuffer();
    buf.append([status(1), status(2)]);
    buf.clear();
    expect(buf.length).toBe(0);
    expect(buf.range(0, 10).t).toEqual([]);
  });

  it("the capture state keeps the pd_status samples of the session channel", () => {
    const pd = new PdCapture();
    const before = pd.statusVersion;
    const e: BackendEvent = {
      type: "pd_status",
      data: { samples: [status(1_000, 0), status(1_005)] },
    };
    pd.handleEvent(e);
    expect(pd.status.length).toBe(2);
    expect(pd.status.range(1_000, 1_005).cc1).toEqual([0, 1.528]);
    expect(pd.statusVersion).toBeGreaterThan(before);
  });

  it("a new session forgets the last session's status samples", () => {
    const pd = new PdCapture();
    pd.handleEvent({ type: "pd_status", data: { samples: [status(1_000)] } });
    pd.handleEvent({
      type: "status",
      data: {
        state: "connected",
        model: "KM003C",
        firmware_version: "2.0.6",
        bootloader_version: "",
        mode: "application",
        warnings: [],
        update_required: false,
        firmware_problems: [],
        serial: "SIM",
        rate_sps: 50,
        rate_locked: false,
        session_id: 2,
      },
    });
    expect(pd.status.length).toBe(0);
  });
});

// ---- AC-TGE83D.5: dense markers merge into a count badge ----

describe("clusterMarkers (AC-TGE83D.5)", () => {
  it("merges markers closer than 4 px into one counted cluster", () => {
    expect(PD_MARKER_MERGE_PX).toBe(4);
    const clusters = clusterMarkers([marker(0, 100), marker(1, 103.9)], scale(0, 1));
    expect(clusters).toHaveLength(1);
    expect(clusters[0]!.count).toBe(2);
    expect(clusters[0]!.markers.map((m) => m.index)).toEqual([0, 1]);
  });

  it("keeps markers 4 px or more apart separate, each at its own position", () => {
    const toPx = scale(0, 1);
    const clusters = clusterMarkers([marker(0, 100, "attach_detach"), marker(1, 104)], toPx);
    expect(clusters.map((c) => c.count)).toEqual([1, 1]);
    expect(clusters.map((c) => c.px)).toEqual([100, 104]);
    expect(clusters.map((c) => c.markers[0]!.kind)).toEqual(["attach_detach", "negotiation"]);
  });

  it("zooming in splits a badge into separate markers", () => {
    // Source_Capabilities, GoodCRC, Request 10 ms apart.
    const ms = [marker(1, 1_280), marker(2, 1_281), marker(3, 1_290)];
    // A 10 s view over 800 px: 0.08 px per ms, all three within 1 px.
    const wide = clusterMarkers(ms, scale(0, 800 / 10_000));
    expect(wide).toHaveLength(1);
    expect(wide[0]!.count).toBe(3);
    // A 100 ms view over 800 px: 8 px per ms, every marker apart.
    const zoomed = clusterMarkers(ms, scale(1_250, 800 / 100));
    expect(zoomed.map((c) => c.count)).toEqual([1, 1, 1]);
    expect(zoomed.map((c) => c.markers[0]!.index)).toEqual([1, 2, 3]);
  });

  it("counts every marker exactly once", () => {
    const many = Array.from({ length: 1_000 }, (_, i) => marker(i, i * 1.5));
    const clusters = clusterMarkers(many, scale(0, 1));
    expect(clusters.reduce((n, c) => n + c.count, 0)).toBe(1_000);
    expect(clusters.flatMap((c) => c.markers.map((m) => m.index))).toEqual(
      many.map((m) => m.index),
    );
    // Neighbouring clusters are at least 4 px apart.
    for (let i = 1; i < clusters.length; i++) {
      const prevLast = clusters[i - 1]!.markers.at(-1)!;
      const first = clusters[i]!.markers[0]!;
      expect(first.ms - prevLast.ms).toBeGreaterThanOrEqual(PD_MARKER_MERGE_PX);
    }
  });
});

describe("drawn marker shapes never overlap (AC-TGE83D.5)", () => {
  it("a marker that would land inside a neighbour's count badge joins it", async () => {
    const { PD_MARKER_R, pdClusterHalfWidth } = await import("./pdTimeline");
    // Three markers make a badge at 100 px; one more 6 px past the last is still
    // inside that badge.
    const ms = [marker(0, 100), marker(1, 101), marker(2, 102), marker(3, 108)];
    const clusters = clusterMarkers(ms, scale(0, 1));
    expect(clusters).toHaveLength(1);
    expect(clusters[0]!.count).toBe(4);
    // Lone markers are drawn small enough that 4 px apart they do not overlap.
    expect(2 * PD_MARKER_R).toBeLessThanOrEqual(PD_MARKER_MERGE_PX);
    expect(pdClusterHalfWidth(clusters[0]!)).toBeGreaterThan(PD_MARKER_R);
  });

  it("neighbouring clusters' drawn extents are apart", async () => {
    const { PD_MARKER_R, pdClusterHalfWidth } = await import("./pdTimeline");
    const many = Array.from({ length: 400 }, (_, i) => marker(i, (i * 37) % 997));
    many.sort((a, b) => a.ms - b.ms);
    const clusters = clusterMarkers(many, scale(0, 1));
    for (let i = 1; i < clusters.length; i++) {
      const prev = clusters[i - 1]!;
      const next = clusters[i]!;
      expect(next.px - PD_MARKER_R).toBeGreaterThanOrEqual(prev.px + pdClusterHalfWidth(prev));
    }
  });
});
