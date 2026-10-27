/** FR 1HHT03: one store per connected meter, the tile grid and the expanded tile. */

import { describe, expect, it } from "vitest";
import type { BackendEvent, DeviceListing, Sample, StatsSnapshot } from "./backend";
import { Meters } from "./meters.svelte";
import { Session } from "./session.svelte";

const A = "usb:1:7";
const B = "demo:pd-9v-2a";

const NO_STATS: StatsSnapshot = {
  elapsed_ms: 0,
  samples: 0,
  charge_ah: 0,
  energy_wh: 0,
  vbus: null,
  ibus: null,
  power: null,
};

function listing(id: string, serial: string, demo: string | null = null): DeviceListing {
  return {
    id,
    label: demo ? `Demo device — ${demo}` : `KM003C — ${serial}`,
    model: "KM003C",
    serial,
    bus: demo ? null : "1",
    address: demo ? null : 7,
    demo_scenario: demo,
    mode: "application",
  };
}

function connected(serial: string, session_id: number): BackendEvent {
  return {
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
      serial,
      rate_sps: 50,
      rate_locked: false,
      session_id,
    },
  };
}

function batch(vbus_v: number): BackendEvent {
  const s: Sample = { t_ms: 1, vbus_v, ibus_a: 1, cc1_v: 1.68, cc2_v: 0, dp_v: 0.6, dm_v: 0 };
  return { type: "batch", data: { samples: [s], temp_c: 30, stats: NO_STATS } };
}

/** Two meters, both streaming. */
function two(): Meters {
  const m = new Meters();
  m.add(listing(A, "SERIAL-0007"));
  m.add(listing(B, "123457", "pd-9v-2a"));
  m.handleEvent(A, connected("SERIAL-0007", 1));
  m.handleEvent(B, connected("123457", 2));
  return m;
}

describe("Meters (AC-1HHT03.1)", () => {
  it("adds a store per meter, in the order they were added", () => {
    const m = two();
    expect(m.ids).toEqual([A, B]);
    expect(m.get(A)?.session.state).toBe("streaming");
    expect(m.get(B)?.session.state).toBe("streaming");
    expect(m.get(A)?.session).not.toBe(m.get(B)?.session);
  });

  it("adding a meter twice keeps one store", () => {
    const m = two();
    const before = m.get(A)?.session;
    m.add(listing(A, "SERIAL-0007"));
    expect(m.ids).toEqual([A, B]);
    expect(m.get(A)?.session).toBe(before);
  });
});

describe("Meters route events (AC-1HHT03.3)", () => {
  it("a meter's events change only its own store", () => {
    const m = two();
    m.handleEvent(A, batch(5.01));
    m.handleEvent(B, batch(9.02));
    expect(m.get(A)?.session.latest?.vbus_v).toBe(5.01);
    expect(m.get(B)?.session.latest?.vbus_v).toBe(9.02);
    m.handleEvent(B, {
      type: "status",
      data: { state: "rate", rate_sps: 10, rate_locked: false },
    });
    expect(m.get(B)?.session.rateSps).toBe(10);
    expect(m.get(A)?.session.rateSps).toBe(50);
  });

  it("ignores events for a meter it does not hold", () => {
    const m = two();
    m.handleEvent("usb:9:9", batch(20));
    expect(m.get(A)?.session.latest).toBeNull();
    expect(m.get(B)?.session.latest).toBeNull();
    expect(m.ids).toEqual([A, B]);
  });
});

describe("Meters when one meter goes (AC-1HHT03.4)", () => {
  it("one meter's unplug leaves the other streaming", () => {
    const m = two();
    m.handleEvent(A, {
      type: "status",
      data: {
        state: "disconnected",
        error: { kind: "disconnected", message: "The meter was unplugged.", hint: null },
      },
    });
    m.handleEvent(A, { type: "status", data: { state: "waiting", serial: "SERIAL-0007" } });
    expect(m.get(A)?.session.state).toBe("waiting");
    expect(m.get(B)?.session.state).toBe("streaming");
    expect(m.get(B)?.session.error).toBeNull();
    m.handleEvent(B, batch(9.0));
    expect(m.get(B)?.session.latest?.vbus_v).toBe(9.0);
  });

  it("removing a meter keeps the others", () => {
    const m = two();
    m.remove(A);
    expect(m.ids).toEqual([B]);
    expect(m.get(A)).toBeUndefined();
    expect(m.get(B)?.session.state).toBe("streaming");
  });
});

describe("Meters grid and expanded tile (AC-1HHT03.2)", () => {
  it("shows a grid only for two meters or more", () => {
    const m = new Meters();
    m.add(listing(A, "SERIAL-0007"));
    expect(m.showGrid).toBe(false);
    m.add(listing(B, "123457", "pd-9v-2a"));
    expect(m.showGrid).toBe(true);
  });

  it("expands one tile to the full single-meter view and back", () => {
    const m = two();
    m.expand(B);
    expect(m.expandedId).toBe(B);
    expect(m.showGrid).toBe(false);
    expect(m.focused?.listing.id).toBe(B);
    m.collapse();
    expect(m.expandedId).toBeNull();
    expect(m.showGrid).toBe(true);
  });

  it("refuses to expand a meter it does not hold", () => {
    const m = two();
    m.expand("usb:9:9");
    expect(m.expandedId).toBeNull();
  });

  it("removing the expanded meter goes back to the grid of the rest", () => {
    const m = two();
    m.add(listing("usb:1:8", "SERIAL-0008"));
    m.expand(A);
    m.remove(A);
    expect(m.expandedId).toBeNull();
    expect(m.showGrid).toBe(true);
  });

  it("with a single meter the full view is that meter", () => {
    const m = new Meters();
    m.add(listing(A, "SERIAL-0007"));
    expect(m.focused?.listing.id).toBe(A);
  });
});

const UNPLUGGED: BackendEvent = {
  type: "status",
  data: {
    state: "disconnected",
    error: { kind: "disconnected", message: "The meter was unplugged.", hint: null },
  },
};
const ASKED_TO_END: BackendEvent = { type: "status", data: { state: "disconnected", error: null } };

/** Lets queued microtasks run, as between two channel messages. */
const flush = (): Promise<void> => new Promise((resolve) => setTimeout(resolve, 0));

describe("Meters follow the full view's meter through an unplug (AC-TGE839.2, review blocker)", () => {
  it("the full view keeps its meter from the unplug through the wait to the replug", async () => {
    const m = new Meters();
    const session = new Session();
    m.fullViewId = A;
    m.syncFullView(session, listing(A, "SERIAL-0007"));
    session.handleEvent(connected("SERIAL-0007", 1));
    m.syncFullView(session, listing(A, "SERIAL-0007"));
    expect(m.get(A)?.session).toBe(session);

    session.handleEvent(UNPLUGGED);
    m.fullViewEvent(UNPLUGGED);
    m.syncFullView(session, undefined);
    await flush();
    expect(m.inFullView(A)).toBe(true);
    expect(m.get(A)?.session).toBe(session);

    session.handleEvent({ type: "status", data: { state: "waiting", serial: "SERIAL-0007" } });
    m.fullViewEvent({ type: "status", data: { state: "waiting", serial: "SERIAL-0007" } });
    // The picker follows the meter to its new address; the view keeps the id it connected.
    session.selectedId = "usb:1:9";
    m.syncFullView(session, undefined);
    expect(m.inFullView(A)).toBe(true);
    expect(m.get(A)?.session).toBe(session);

    session.handleEvent(connected("SERIAL-0007", 1));
    m.fullViewEvent(connected("SERIAL-0007", 1));
    m.syncFullView(session, undefined);
    expect(m.inFullView(A)).toBe(true);
    expect(session.state).toBe("streaming");
  });

  it("a disconnect the user asked for ends the full view's meter", () => {
    const m = new Meters();
    const session = new Session();
    m.fullViewId = A;
    session.handleEvent(connected("SERIAL-0007", 1));
    m.syncFullView(session, listing(A, "SERIAL-0007"));
    session.handleEvent(ASKED_TO_END);
    m.fullViewEvent(ASKED_TO_END);
    m.syncFullView(session, undefined);
    expect(m.inFullView(A)).toBe(false);
    expect(m.get(A)).toBeUndefined();
  });

  it("another meter taking the view drops the old entry of the view's store", () => {
    const m = new Meters();
    const session = new Session();
    m.fullViewId = A;
    session.handleEvent(connected("SERIAL-0007", 1));
    m.syncFullView(session, listing(A, "SERIAL-0007"));
    m.fullViewId = B;
    m.syncFullView(session, listing(B, "123457", "pd-9v-2a"));
    expect(m.ids).toEqual([B]);
    expect(m.get(B)?.session).toBe(session);
  });
});

describe("Meters keep an unplugged tile (review major 1, minor 4)", () => {
  it("an unplugged tile stays, waits, and streams again on the replug", async () => {
    const m = two();
    m.handleTileEvent(B, UNPLUGGED);
    await flush();
    expect(m.ids).toEqual([A, B]);
    expect(m.get(B)?.session.state).toBe("error");
    m.handleTileEvent(B, { type: "status", data: { state: "waiting", serial: "123457" } });
    expect(m.get(B)?.session.state).toBe("waiting");
    m.handleTileEvent(B, connected("123457", 2));
    expect(m.get(B)?.session.state).toBe("streaming");
    expect(m.ids).toEqual([A, B]);
  });

  it("a disconnect the user asked for takes the tile away", () => {
    const m = two();
    m.handleTileEvent(B, ASKED_TO_END);
    expect(m.ids).toEqual([A]);
  });

  it("a recording that ends away from the full view leaves no parked recording", () => {
    const m = two();
    m.park(B, { rows: 10, elapsed_ms: 1000 });
    m.handleTileEvent(B, {
      type: "recording",
      data: { state: "stopped", rows: 12, gaps: [], disconnected: true },
    });
    expect(m.unpark(B)).toBeUndefined();

    m.park(A, { rows: 10, elapsed_ms: 1000 });
    m.handleTileEvent(A, {
      type: "recording",
      data: {
        state: "failed",
        rows: 12,
        error: { kind: "internal", message: "Disk full.", hint: null },
      },
    });
    expect(m.unpark(A)).toBeUndefined();
  });

  it("a parked recording comes back once, and goes with its meter", () => {
    const m = two();
    m.park(B, { rows: 10, elapsed_ms: 1000 });
    expect(m.unpark(B)).toEqual({ rows: 10, elapsed_ms: 1000 });
    expect(m.unpark(B)).toBeUndefined();
    m.park(B, { rows: 10, elapsed_ms: 1000 });
    m.remove(B);
    expect(m.unpark(B)).toBeUndefined();
  });
});
