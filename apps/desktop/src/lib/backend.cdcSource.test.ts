/** The measurement source on connect over Tauri IPC (FR 1HHT02, AC .4): the CDC
 * ADCNEW stream is asked for with `source: "cdc"`; the queue stays the default. */

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

import { connect, MEASUREMENT_SOURCES, type BackendEvent, type MeasurementSource } from "./backend";

beforeEach(() => {
  invoke.mockReset();
  channels.length = 0;
});

describe("connect with a measurement source (AC-1HHT02.4)", () => {
  it("asks for the CDC stream with source: cdc", async () => {
    invoke.mockResolvedValue(undefined);
    const events: BackendEvent[] = [];
    await connect("demo:load-5v-1a", 50, (e) => events.push(e), "cdc");
    expect(invoke).toHaveBeenCalledTimes(1);
    expect(invoke).toHaveBeenCalledWith(
      "connect",
      expect.objectContaining({ deviceId: "demo:load-5v-1a", rateSps: 50, source: "cdc" }),
    );
    const ev: BackendEvent = {
      type: "status",
      data: { state: "disconnected", error: null },
    };
    channels[0]?.onmessage(ev);
    expect(events).toEqual([ev]);
  });

  it("asks for the queue when the queue is picked", async () => {
    invoke.mockResolvedValue(undefined);
    await connect("demo:idle", 10, () => {}, "queue");
    const args = invoke.mock.calls[0]?.[1] as Record<string, unknown>;
    expect(args.source ?? "queue").toBe("queue");
  });

  it("offers the queue first and the CDC stream second, each with a label", () => {
    const values: MeasurementSource[] = MEASUREMENT_SOURCES.map((s) => s.value);
    expect(values).toEqual(["queue", "cdc"]);
    for (const s of MEASUREMENT_SOURCES) expect(s.label.length).toBeGreaterThan(0);
  });
});
