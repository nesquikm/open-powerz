import { describe, expect, it } from "vitest";
import type { BackendEvent, DeviceListing, Sample, StatsSnapshot, UiError } from "./backend";
import { Session } from "./session.svelte";

const NO_STATS: StatsSnapshot = {
  elapsed_ms: 0,
  samples: 0,
  charge_ah: 0,
  energy_wh: 0,
  vbus: null,
  ibus: null,
  power: null,
};

function sample(t_ms: number, vbus_v = 5): Sample {
  return { t_ms, vbus_v, ibus_a: 1, cc1_v: 1.68, cc2_v: 0, dp_v: 0.6, dm_v: 0 };
}

function connected(rate_locked = false, rate_sps: number | null = 50): BackendEvent {
  return {
    type: "status",
    data: {
      state: "connected",
      model: "KM003C",
      firmware_version: "2.0.6",
      serial: "12345678",
      rate_sps,
      rate_locked,
      session_id: 1,
    },
  };
}

function listing(id: string): DeviceListing {
  return {
    id,
    label: `KM003C — ${id}`,
    model: "KM003C",
    serial: id,
    bus: "1",
    address: 1,
    demo_scenario: null,
  };
}

const udev: UiError = {
  kind: "permission_denied",
  message: "The app may not open the meter.",
  hint: "Install the udev rule: sudo cp packaging/linux/60-powerz.rules /etc/udev/rules.d/",
};

describe("Session state badge (AC-AH2AEK.3)", () => {
  it("starts disconnected", () => {
    expect(new Session().state).toBe("disconnected");
  });

  it("moves through connecting to streaming", () => {
    const s = new Session();
    s.beginConnect();
    expect(s.state).toBe("connecting");
    s.handleEvent(connected());
    expect(s.state).toBe("streaming");
    expect(s.rateSps).toBe(50);
  });

  it("shows rate locked when the meter refused authentication", () => {
    const s = new Session();
    s.beginConnect();
    s.handleEvent(connected(true));
    expect(s.state).toBe("rate_locked");
  });

  it("follows rate updates", () => {
    const s = new Session();
    s.handleEvent(connected());
    s.handleEvent({ type: "status", data: { state: "rate", rate_sps: 1000, rate_locked: false } });
    expect(s.rateSps).toBe(1000);
    expect(s.state).toBe("streaming");
    s.handleEvent({ type: "status", data: { state: "rate", rate_sps: 1000, rate_locked: true } });
    expect(s.state).toBe("rate_locked");
  });

  it("returns to disconnected on a requested disconnect", () => {
    const s = new Session();
    s.handleEvent(connected());
    s.handleEvent({ type: "status", data: { state: "disconnected", error: null } });
    expect(s.state).toBe("disconnected");
    expect(s.error).toBeNull();
  });

  it("shows error when the session ends with an error", () => {
    const s = new Session();
    s.handleEvent(connected());
    s.handleEvent({ type: "status", data: { state: "disconnected", error: udev } });
    expect(s.state).toBe("error");
    expect(s.error).toEqual(udev);
  });

  it("shows error when connecting fails", () => {
    const s = new Session();
    s.beginConnect();
    s.failConnect(udev);
    expect(s.state).toBe("error");
    expect(s.error).toEqual(udev);
  });
});

describe("Session device list (AC-AH2AEK.3)", () => {
  it("follows hotplug updates and keeps the selection while present", () => {
    const s = new Session();
    s.setDevices([listing("a"), listing("b")]);
    expect(s.devices.map((d) => d.id)).toEqual(["a", "b"]);
    expect(s.selectedId).toBe("a");
    s.select("b");
    s.setDevices([listing("b"), listing("c")]);
    expect(s.devices.map((d) => d.id)).toEqual(["b", "c"]);
    expect(s.selectedId).toBe("b");
  });

  it("falls back when the selected device is unplugged", () => {
    const s = new Session();
    s.setDevices([listing("a"), listing("b")]);
    s.select("b");
    s.setDevices([listing("a")]);
    expect(s.selectedId).toBe("a");
    s.setDevices([]);
    expect(s.selectedId).toBeNull();
  });
});

describe("Session readout values (AC-AH2AEK.2, AC-AH2AEK.9)", () => {
  it("keeps the latest sample of each batch", () => {
    const s = new Session();
    s.handleEvent(connected());
    s.handleEvent({
      type: "batch",
      data: { samples: [sample(1, 5), sample(2, 9)], temp_c: 30, stats: NO_STATS },
    });
    expect(s.latest?.vbus_v).toBe(9);
    expect(s.tempC).toBe(30);
  });

  it("keeps the last sample across an empty heartbeat batch", () => {
    const s = new Session();
    s.handleEvent(connected());
    s.handleEvent({
      type: "batch",
      data: { samples: [sample(1, 12)], temp_c: 30, stats: NO_STATS },
    });
    s.handleEvent({ type: "batch", data: { samples: [], temp_c: 30, stats: NO_STATS } });
    expect(s.latest?.vbus_v).toBe(12);
  });

  it("drops a stale temperature once the meter stops reporting it", () => {
    const s = new Session();
    s.handleEvent(connected());
    s.handleEvent({ type: "batch", data: { samples: [sample(1)], temp_c: 31.5, stats: NO_STATS } });
    s.handleEvent({ type: "batch", data: { samples: [sample(2)], temp_c: null, stats: NO_STATS } });
    expect(s.tempC).toBeNull();
  });

  it("clears the readout when disconnected", () => {
    const s = new Session();
    s.handleEvent(connected());
    s.handleEvent({ type: "batch", data: { samples: [sample(1)], temp_c: 31.5, stats: NO_STATS } });
    s.handleEvent({ type: "status", data: { state: "disconnected", error: null } });
    expect(s.latest).toBeNull();
    expect(s.tempC).toBeNull();
  });
});

describe("Session error banner (AC-AH2AEK.4)", () => {
  it("keeps the backend message and hint and is dismissible", () => {
    const s = new Session();
    s.handleEvent(connected());
    s.handleEvent({ type: "error", data: udev });
    expect(s.error?.message).toBe(udev.message);
    expect(s.error?.hint).toContain("udev");
    s.dismissError();
    expect(s.error).toBeNull();
  });

  it("clears an old error when a new connection starts", () => {
    const s = new Session();
    s.failConnect(udev);
    s.beginConnect();
    expect(s.error).toBeNull();
    expect(s.state).toBe("connecting");
  });
});

describe("Session lifecycle edges (review round 1)", () => {
  const batch = (vbus: number): BackendEvent => ({
    type: "batch",
    data: {
      samples: [{ t_ms: 1, vbus_v: vbus, ibus_a: 0, cc1_v: 0, cc2_v: 0, dp_v: 0, dm_v: 0 }],
      temp_c: 30,
      stats: NO_STATS,
    },
  });
  const connected: BackendEvent = {
    type: "status",
    data: {
      state: "connected",
      model: "KM003C",
      firmware_version: "2.0.6",
      serial: "1",
      rate_sps: 1000,
      rate_locked: false,
      session_id: 1,
    },
  };
  const disconnected: BackendEvent = {
    type: "status",
    data: { state: "disconnected", error: null },
  };

  it("forgets the session rate on disconnect", () => {
    const s = new Session();
    s.beginConnect();
    s.handleEvent(connected);
    s.handleEvent(disconnected);
    expect(s.rateSps).toBeNull();
  });

  it("ignores batches that arrive after the session ended", () => {
    const s = new Session();
    s.beginConnect();
    s.handleEvent(connected);
    s.handleEvent(disconnected);
    s.handleEvent(batch(5));
    expect(s.latest).toBeNull();
    expect(s.tempC).toBeNull();
  });

  it("reports a side error without changing the connection state", () => {
    const s = new Session();
    s.beginConnect();
    s.handleEvent(connected);
    s.reportError({ kind: "usage", message: "No meter is connected.", hint: null });
    expect(s.state).toBe("streaming");
    expect(s.error?.message).toBe("No meter is connected.");
  });

  it("does not offer connect or disconnect while connecting", () => {
    const s = new Session();
    s.selectedId = "demo:idle";
    expect(s.canToggleConnect).toBe(true);
    s.beginConnect();
    expect(s.canToggleConnect).toBe(false);
    s.handleEvent(connected);
    expect(s.canToggleConnect).toBe(true);
  });
});

// ---- FR AH2AEJ: the stats snapshot riding each batch ----

describe("Session stats (AC-AH2AEJ.6)", () => {
  const snap = {
    elapsed_ms: 2000,
    samples: 100,
    charge_ah: -0.12,
    energy_wh: -0.6,
    vbus: { min: 4.9, max: 5.1, avg: 5 },
    ibus: { min: -0.3, max: -0.1, avg: -0.2 },
    power: { min: -1.5, max: -0.5, avg: -1 },
  };

  it("starts without stats", () => {
    expect(new Session().stats).toBeNull();
  });

  it("takes the stats of every batch, heartbeats included", () => {
    const s = new Session();
    s.handleEvent(connected());
    s.handleEvent({ type: "batch", data: { samples: [sample(1)], temp_c: null, stats: snap } });
    expect(s.stats).toEqual(snap);
    const later = { ...snap, samples: 101, charge_ah: -0.13 };
    s.handleEvent({ type: "batch", data: { samples: [], temp_c: null, stats: later } });
    expect(s.stats).toEqual(later);
  });
});

describe("Session stats after a reset (review round 1)", () => {
  it("blanks the stats until the next batch brings fresh ones", () => {
    const s = new Session();
    s.handleEvent(connected());
    s.handleEvent({
      type: "batch",
      data: { samples: [sample(1)], temp_c: 30, stats: { ...NO_STATS, samples: 7 } },
    });
    s.statsReset();
    expect(s.stats).toBeNull();
    s.handleEvent({ type: "batch", data: { samples: [sample(2)], temp_c: 30, stats: NO_STATS } });
    expect(s.stats).toEqual(NO_STATS);
  });
});
