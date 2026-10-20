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
  type BackendEvent,
  type DeviceListing,
} from "./backend";

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
    const ev: BackendEvent = { type: "batch", data: { samples: [], temp_c: null } };
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
