import { beforeEach, describe, expect, it, vi } from "vitest";

const { invoke, channels } = vi.hoisted(() => ({
  invoke: vi.fn(),
  channels: [] as { onmessage: (msg: unknown) => void }[],
}));

vi.mock("@tauri-apps/api/core", () => {
  class Channel {
    onmessage: (msg: unknown) => void = () => {};
    constructor() {
      channels.push(this);
    }
  }
  return { invoke, Channel, isTauri: () => true };
});

import {
  appInfo,
  connect,
  disconnect,
  listDevices,
  setRate,
  toUiError,
  INTERNAL_MESSAGE,
  watchDevices,
  type AttachedSession,
  type BackendEvent,
  type DeviceListing,
} from "./backend";

const NO_STATS: StatsSnapshot = {
  elapsed_ms: 0,
  samples: 0,
  charge_ah: 0,
  energy_wh: 0,
  vbus: null,
  ibus: null,
  power: null,
};

const meter: DeviceListing = {
  id: "usb:1:4",
  label: "KM003C — 12345678",
  model: "KM003C",
  serial: "12345678",
  bus: "1",
  address: 4,
  demo_scenario: null,
};

beforeEach(() => {
  invoke.mockReset();
  channels.length = 0;
});

describe("device list (AC-AH2AEK.3)", () => {
  it("lists devices through list_devices", async () => {
    invoke.mockResolvedValue([meter]);
    await expect(listDevices()).resolves.toEqual([meter]);
    expect(invoke).toHaveBeenCalledWith("list_devices");
  });

  it("forwards hotplug updates from the watch channel", async () => {
    invoke.mockResolvedValue(undefined);
    const seen: DeviceListing[][] = [];
    await watchDevices((list) => seen.push(list));
    expect(invoke).toHaveBeenCalledWith("watch_devices", { onChange: channels[0] });
    channels[0]?.onmessage([meter]);
    channels[0]?.onmessage([]);
    expect(seen).toEqual([[meter], []]);
  });

  it("stops delivering hotplug updates once unsubscribed", async () => {
    invoke.mockResolvedValue(undefined);
    const seen: DeviceListing[][] = [];
    const stop = await watchDevices((list) => seen.push(list));
    stop();
    channels.at(-1)?.onmessage([meter]);
    expect(seen).toEqual([]);
  });
});

describe("session commands", () => {
  it("connects with camelCase args and forwards events", async () => {
    invoke.mockResolvedValue(undefined);
    const events: BackendEvent[] = [];
    await connect("demo:steady", 50, (ev) => events.push(ev));
    expect(invoke).toHaveBeenCalledWith("connect", {
      deviceId: "demo:steady",
      rateSps: 50,
      onEvent: channels[0],
    });
    const ev: BackendEvent = {
      type: "batch",
      data: { samples: [], temp_c: null, stats: NO_STATS },
    };
    channels[0]?.onmessage(ev);
    expect(events).toEqual([ev]);
  });

  it("disconnects and sets the rate", async () => {
    invoke.mockResolvedValue(undefined);
    await disconnect();
    await setRate(1000);
    expect(invoke).toHaveBeenNthCalledWith(1, "disconnect");
    expect(invoke).toHaveBeenNthCalledWith(2, "set_rate", { rateSps: 1000 });
  });
});

describe("errors (AC-AH2AEK.4)", () => {
  const udev = {
    kind: "permission_denied",
    message: "The app may not open the meter.",
    hint: "Install the udev rule: sudo cp packaging/linux/60-powerz.rules /etc/udev/rules.d/",
  };

  it("rejects with the backend UiError, message and hint intact", async () => {
    invoke.mockRejectedValue(udev);
    await expect(connect("usb:1:4", 50, () => {})).rejects.toEqual(udev);
  });

  it("turns a plain string rejection into an internal UiError", () => {
    // Raw text never reaches the banner: it is plain language or nothing.
    expect(toUiError("boom")).toEqual({ kind: "internal", message: INTERNAL_MESSAGE, hint: null });
  });

  it("turns an Error into an internal UiError", () => {
    expect(toUiError(new Error("Cannot read properties of undefined"))).toEqual({
      kind: "internal",
      message: INTERNAL_MESSAGE,
      hint: null,
    });
  });

  it("passes a UiError through", () => {
    expect(toUiError(udev)).toEqual(udev);
  });
});

describe("appInfo (AC-AH2AEK.8)", () => {
  it("returns the app and protocol-library versions", async () => {
    invoke.mockResolvedValue({ app_version: "0.2.0", protocol_version: "0.1.5" });
    await expect(appInfo()).resolves.toEqual({ appVersion: "0.2.0", protocolVersion: "0.1.5" });
    expect(invoke).toHaveBeenCalledWith("app_info");
  });
});

// ---- FR 0P7HSJ: recording and export commands ----

describe("recording commands (AC-0P7HSJ.3, AC-0P7HSJ.5)", () => {
  it("starts a recording with the proposed name and returns the chosen path", async () => {
    const { startRecording } = await import("./backend");
    invoke.mockResolvedValue("/home/u/powerz-20261021-090507.csv");
    await expect(startRecording("powerz-20261021-090507.csv")).resolves.toBe(
      "/home/u/powerz-20261021-090507.csv",
    );
    expect(invoke).toHaveBeenCalledWith("start_recording", {
      suggestedName: "powerz-20261021-090507.csv",
    });
  });

  it("resolves to null when the save dialog is cancelled", async () => {
    const { startRecording } = await import("./backend");
    invoke.mockResolvedValue(null);
    await expect(startRecording("powerz.csv")).resolves.toBeNull();
  });

  it("stops a recording and returns its summary", async () => {
    const { stopRecording } = await import("./backend");
    const summary = { rows: 12, gaps: [], disconnected: false };
    invoke.mockResolvedValue(summary);
    await expect(stopRecording()).resolves.toEqual(summary);
    expect(invoke).toHaveBeenCalledWith("stop_recording");
  });

  it("exports the session with a progress channel", async () => {
    const { exportSession } = await import("./backend");
    invoke.mockResolvedValue({ path: "/tmp/x.csv", rows: 3 });
    const seen: unknown[] = [];
    const result = exportSession("powerz-x.csv", (p) => seen.push(p));
    expect(invoke).toHaveBeenCalledWith("export_session", {
      suggestedName: "powerz-x.csv",
      onProgress: channels.at(-1),
    });
    channels.at(-1)?.onmessage({ done: 1, total: 3 });
    await expect(result).resolves.toEqual({ path: "/tmp/x.csv", rows: 3 });
    expect(seen).toEqual([{ done: 1, total: 3 }]);
  });

  it("forwards recording events from the session channel", async () => {
    invoke.mockResolvedValue(undefined);
    const events: BackendEvent[] = [];
    await connect("demo:idle", 50, (ev) => events.push(ev));
    const ev: BackendEvent = {
      type: "recording",
      data: { state: "progress", rows: 4, elapsed_ms: 1000 },
    };
    channels.at(-1)?.onmessage(ev);
    expect(events).toEqual([ev]);
  });
});

// ---- FR AH2AEJ: history and stats commands ----

import { clearHistory, historyRange, resetStats, stats, type StatsSnapshot } from "./backend";

describe("history and stats commands (AC-AH2AEJ.2, AC-AH2AEJ.4, AC-AH2AEJ.5)", () => {
  const snap: StatsSnapshot = {
    elapsed_ms: 1000,
    samples: 50,
    charge_ah: -0.001,
    energy_wh: -0.005,
    vbus: { min: 4.9, max: 5.1, avg: 5 },
    ibus: { min: -0.3, max: -0.1, avg: -0.2 },
    power: { min: -1.5, max: -0.5, avg: -1 },
  };

  it("queries a history range per series", async () => {
    const answer = [{ series: "vbus", buckets: [{ t_ms: 0, min: 4.9, max: 5.1 }] }];
    invoke.mockResolvedValue(answer);
    await expect(historyRange(["vbus", "ibus"], 0, 60_000, 2000)).resolves.toEqual(answer);
    expect(invoke).toHaveBeenCalledWith("history_range", {
      series: ["vbus", "ibus"],
      t0Ms: 0,
      t1Ms: 60_000,
      buckets: 2000,
    });
  });

  it("history_gaps: asks for the breaks between runs in a range (AC-272WSN.3)", async () => {
    const { historyGaps } = await import("./backend");
    const answer = [{ from_ms: 990, to_ms: 2_400_990 }];
    invoke.mockResolvedValue(answer);
    await expect(historyGaps(0, 60_000)).resolves.toEqual(answer);
    expect(invoke).toHaveBeenCalledWith("history_gaps", { t0Ms: 0, t1Ms: 60_000 });
  });

  it("reads, resets and clears", async () => {
    invoke.mockResolvedValue(snap);
    await expect(stats()).resolves.toEqual(snap);
    expect(invoke).toHaveBeenCalledWith("stats");
    invoke.mockResolvedValue(undefined);
    await resetStats();
    expect(invoke).toHaveBeenCalledWith("reset_stats");
    await clearHistory();
    expect(invoke).toHaveBeenCalledWith("clear_history");
  });
});

// ---- FR VEDDFD: re-attach to the running session on load ----

describe("attach (AC-VEDDFD.1, AC-VEDDFD.2)", () => {
  it("asks the backend over attach with a channel and returns null when nothing runs", async () => {
    const { attach } = await import("./backend");
    invoke.mockResolvedValue(null);
    await expect(attach(() => {})).resolves.toBeNull();
    expect(invoke).toHaveBeenCalledWith("attach", { onEvent: channels[0] });
  });

  it("returns the running session's snapshot and forwards its events to the new page", async () => {
    const { attach } = await import("./backend");
    const snapshot: AttachedSession = {
      device_id: "usb:1:4",
      model: "KM003C",
      firmware_version: "2.0.6",
      serial: "12345678",
      rate_sps: 50,
      rate_locked: false,
      session_id: 3,
      recording: { rows: 120, elapsed_ms: 2400 },
    };
    invoke.mockResolvedValue(snapshot);
    const events: BackendEvent[] = [];
    await expect(attach((ev) => events.push(ev))).resolves.toEqual(snapshot);
    const ev: BackendEvent = {
      type: "batch",
      data: { samples: [], temp_c: null, stats: NO_STATS },
    };
    channels[0]?.onmessage(ev);
    expect(events).toEqual([ev]);
  });
});
