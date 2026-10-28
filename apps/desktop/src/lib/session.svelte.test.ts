import { describe, expect, it } from "vitest";
import type {
  AttachedSession,
  BackendEvent,
  DeviceListing,
  Sample,
  StatsSnapshot,
  UiError,
} from "./backend";
import { badgeLabel } from "./format";
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
      bootloader_version: "",
      mode: "application",
      warnings: [],
      update_required: false,
      firmware_problems: [],
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
    mode: "application",
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
      bootloader_version: "",
      mode: "application",
      warnings: [],
      update_required: false,
      firmware_problems: [],
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

// ---- FR VEDDFD: the new page adopts the running session ----

describe("Session adopts a running session on load (AC-VEDDFD.2, AC-VEDDFD.5)", () => {
  const snapshot = (over: Partial<AttachedSession> = {}): AttachedSession => ({
    device_id: "usb:1:4",
    model: "KM003C",
    firmware_version: "2.0.6",
    serial: "12345678",
    rate_sps: 50,
    rate_locked: false,
    session_id: 2,
    recording: null,
    ...over,
  });

  it("shows the session as streaming on the picked device, with its rate", () => {
    const s = new Session();
    s.setDevices([listing("usb:1:2"), listing("usb:1:4")]);
    s.adopt(snapshot());
    expect(s.state).toBe("streaming");
    expect(s.selectedId).toBe("usb:1:4");
    expect(s.rateSps).toBe(50);
    expect(s.error).toBeNull();
    expect(s.active).toBe(true);
    // Disconnect is offered, as for a session connected on this page.
    expect(s.canToggleConnect).toBe(true);
  });

  it("counts the adopted meter as connected, so the panels read it (HIL 1HHT04.multi)", () => {
    const s = new Session();
    expect(s.meterConnected).toBe(false);
    s.adopt(snapshot());
    expect(s.meterConnected).toBe(true);
    s.handleEvent({ type: "status", data: { state: "disconnected", error: null } });
    expect(s.meterConnected).toBe(false);
  });

  it("keeps the adopted device picked when the device list arrives afterwards", () => {
    const s = new Session();
    s.adopt(snapshot());
    s.setDevices([listing("usb:1:2"), listing("usb:1:4")]);
    expect(s.selectedId).toBe("usb:1:4");
  });

  it("shows a locked rate as rate locked", () => {
    const s = new Session();
    s.adopt(snapshot({ rate_locked: true, rate_sps: 50 }));
    expect(s.state).toBe("rate_locked");
    expect(s.rateSps).toBe(50);
  });

  it("takes the batches that follow and ends like any session", () => {
    const s = new Session();
    s.adopt(snapshot());
    s.handleEvent({
      type: "batch",
      data: { samples: [sample(10, 5.1)], temp_c: 31, stats: { ...NO_STATS, samples: 900 } },
    });
    expect(s.latest?.vbus_v).toBe(5.1);
    expect(s.stats?.samples).toBe(900);
    s.handleEvent({ type: "status", data: { state: "rate", rate_sps: 10, rate_locked: false } });
    expect(s.rateSps).toBe(10);
    s.handleEvent({ type: "status", data: { state: "disconnected", error: null } });
    expect(s.state).toBe("disconnected");
    expect(s.latest).toBeNull();
  });
});

// ---- FR TGE839: replug reconnects to the meter ----

describe("Session waits for an unplugged meter (AC-TGE839.1–AC-TGE839.5)", () => {
  const SERIAL_A = "SERIAL-REPLUG-A";
  const SERIAL_B = "SERIAL-REPLUG-B";

  function meter(id: string, serial: string): DeviceListing {
    return {
      id,
      label: `KM003C — ${serial}`,
      model: "KM003C",
      serial,
      bus: "1",
      address: Number(id.split(":").at(-1)),
      demo_scenario: null,
      mode: "application",
    };
  }

  function demo(name: string): DeviceListing {
    return {
      id: `demo:${name}`,
      label: `Demo device — ${name}`,
      model: "KM003C",
      serial: "123456",
      bus: null,
      address: null,
      demo_scenario: name,
      mode: "application",
    };
  }

  const DEMOS = [demo("idle"), demo("load-5v-1a")];

  function connectedTo(serial: string, rate_sps = 1000): BackendEvent {
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
        rate_sps,
        rate_locked: false,
        session_id: 7,
      },
    };
  }

  const unplugged: BackendEvent = {
    type: "status",
    data: {
      state: "disconnected",
      error: { kind: "disconnected", message: "The meter was unplugged.", hint: null },
    },
  };

  const waitingForA: BackendEvent = {
    type: "status",
    data: { state: "waiting", serial: SERIAL_A },
  };

  /** Streaming from meter A at usb:1:7, then the cable is pulled. */
  function awayMeter(): Session {
    const s = new Session();
    s.setDevices([meter("usb:1:7", SERIAL_A), ...DEMOS]);
    s.select("usb:1:7");
    s.beginConnect();
    s.handleEvent(connectedTo(SERIAL_A));
    s.handleEvent({
      type: "batch",
      data: { samples: [sample(1_000)], temp_c: null, stats: NO_STATS },
    });
    s.handleEvent(unplugged);
    s.handleEvent(waitingForA);
    // The OS no longer lists the meter.
    s.setDevices(DEMOS);
    return s;
  }

  it("keeps the unplugged meter picked and shows a waiting state (AC-TGE839.1)", () => {
    const s = awayMeter();
    expect(s.state).toBe("waiting");
    expect(s.waitingSerial).toBe(SERIAL_A);
    expect(s.selectedId).toBe("usb:1:7");
    // The picker still shows the away meter, so the selection is visible.
    const picked = s.devices.find((d) => d.id === s.selectedId);
    expect(picked?.serial).toBe(SERIAL_A);
    expect(picked?.demo_scenario).toBeNull();
    expect(badgeLabel(s.state)).toMatch(/waiting/i);
  });

  it("never picks a demo device on its own while the meter is away (AC-TGE839.1)", () => {
    const s = awayMeter();
    s.setDevices([...DEMOS].reverse());
    s.setDevices([]);
    s.setDevices(DEMOS);
    expect(s.selectedId).toBe("usb:1:7");
    expect(s.state).toBe("waiting");
  });

  it("leaves the picker free while waiting, so another device can be chosen (AC-TGE839.5)", () => {
    const s = awayMeter();
    expect(s.active).toBe(false);
  });

  it("follows the meter to its new listing and streams again on reconnect (AC-TGE839.2)", () => {
    const s = awayMeter();
    // The meter comes back at a new USB address.
    s.setDevices([meter("usb:1:9", SERIAL_A), ...DEMOS]);
    expect(s.selectedId).toBe("usb:1:9");
    s.handleEvent(connectedTo(SERIAL_A, 1000));
    expect(s.state).toBe("streaming");
    expect(s.rateSps).toBe(1000);
    expect(s.waitingSerial).toBeNull();
    expect(s.devices.filter((d) => d.serial === SERIAL_A)).toHaveLength(1);
  });

  it("lists a meter with another serial but keeps waiting for the unplugged one (AC-TGE839.4)", () => {
    const s = awayMeter();
    s.setDevices([meter("usb:1:8", SERIAL_B), ...DEMOS]);
    expect(s.devices.some((d) => d.serial === SERIAL_B)).toBe(true);
    expect(s.selectedId).toBe("usb:1:7");
    expect(s.state).toBe("waiting");
    expect(s.waitingSerial).toBe(SERIAL_A);
  });

  it("a demo chosen by hand cancels the wait for the meter (AC-TGE839.5)", () => {
    const s = awayMeter();
    s.select("demo:idle");
    expect(s.selectedId).toBe("demo:idle");
    expect(s.waitingSerial).toBeNull();
    expect(s.state).not.toBe("waiting");
    expect(s.canToggleConnect).toBe(true);
    // The meter coming back does not take the selection from the demo.
    s.setDevices([meter("usb:1:9", SERIAL_A), ...DEMOS]);
    expect(s.selectedId).toBe("demo:idle");
    s.beginConnect();
    expect(s.state).toBe("connecting");
  });

  it("tells the caller when a pick cancels the wait, so the backend can stop waiting too (AC-TGE839.5)", () => {
    expect(awayMeter().select("usb:1:7")).toBe(false);
    expect(awayMeter().select("demo:idle")).toBe(true);
    const idle = new Session();
    idle.setDevices(DEMOS);
    expect(idle.select("demo:load-5v-1a")).toBe(false);
  });

  it("clears the unplug message once the meter streams again (AC-TGE839.2)", () => {
    const s = awayMeter();
    expect(s.error?.kind).toBe("disconnected");
    s.setDevices([meter("usb:1:9", SERIAL_A), ...DEMOS]);
    s.handleEvent(connectedTo(SERIAL_A));
    expect(s.error).toBeNull();
  });

  it("clears a read error that came with the unplug once the meter streams again (HIL 1HHT04.multi)", () => {
    // On macOS the pulled cable can end the session with an I/O error, not an unplug one.
    const s = streamingMeter();
    s.handleEvent({
      type: "status",
      data: {
        state: "disconnected",
        error: {
          kind: "internal",
          message: "Talking to the meter failed.",
          hint: "Replug the meter and try again.",
        },
      },
    });
    s.handleEvent(waitingForA);
    s.setDevices([meter("usb:1:9", SERIAL_A), ...DEMOS]);
    s.handleEvent(connectedTo(SERIAL_A));
    expect(s.error).toBeNull();
    expect(s.state).toBe("streaming");
  });

  /** Streaming from meter A at usb:1:7, before the cable is pulled. */
  function streamingMeter(): Session {
    const s = new Session();
    s.setDevices([meter("usb:1:7", SERIAL_A), ...DEMOS]);
    s.select("usb:1:7");
    s.beginConnect();
    s.handleEvent(connectedTo(SERIAL_A));
    return s;
  }

  it("keeps the meter picked when the new device list beats the status events (AC-TGE839.1)", () => {
    const s = streamingMeter();
    // The hotplug poller runs on its own thread: its list can arrive first.
    s.setDevices(DEMOS);
    expect(s.selectedId).toBe("usb:1:7");
    s.handleEvent(unplugged);
    s.handleEvent(waitingForA);
    expect(s.selectedId).toBe("usb:1:7");
    expect(s.state).toBe("waiting");
    expect(s.devices.find((d) => d.id === s.selectedId)?.serial).toBe(SERIAL_A);
  });

  it("keeps the meter picked when the list lands between disconnect and waiting (AC-TGE839.1)", () => {
    const s = streamingMeter();
    s.handleEvent(unplugged);
    s.setDevices(DEMOS);
    s.handleEvent(waitingForA);
    expect(s.selectedId).toBe("usb:1:7");
    expect(s.state).toBe("waiting");
    // The meter comes back and the selection follows it.
    s.setDevices([meter("usb:1:9", SERIAL_A), ...DEMOS]);
    expect(s.selectedId).toBe("usb:1:9");
  });
});
