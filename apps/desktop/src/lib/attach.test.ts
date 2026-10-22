import { describe, expect, it, vi } from "vitest";
import { attachSession } from "./attach";
import type { AttachedSession, BackendEvent } from "./backend";

const SNAPSHOT: AttachedSession = {
  device_id: "demo:pd-9v-2a",
  model: "KM003C",
  firmware_version: "1.0",
  serial: "DEMO",
  rate_sps: 50,
  rate_locked: false,
  session_id: 1,
  recording: null,
};

const DISCONNECTED: BackendEvent = {
  type: "status",
  data: { state: "disconnected", error: null },
} as BackendEvent;

describe("attachSession (FR VEDDFD)", () => {
  it("passes events that arrive before the snapshot only after adopting it (AC-VEDDFD.2)", async () => {
    const order: string[] = [];
    let send: ((e: BackendEvent) => void) | undefined;
    const done = attachSession(
      (onEvent) => {
        send = onEvent;
        // The backend hands the events over first, then answers.
        onEvent(DISCONNECTED);
        return Promise.resolve(SNAPSHOT);
      },
      (e) => order.push(`event:${e.type}`),
      () => order.push("adopt"),
    );
    await done;
    expect(order).toEqual(["adopt", "event:status"]);
    // Later events go straight through.
    send?.(DISCONNECTED);
    expect(order).toEqual(["adopt", "event:status", "event:status"]);
  });

  it("with no session, passes held events on without adopting (AC-VEDDFD.1)", async () => {
    const onEvent = vi.fn();
    const adopt = vi.fn();
    await attachSession(
      (send) => {
        send(DISCONNECTED);
        return Promise.resolve(null);
      },
      onEvent,
      adopt,
    );
    expect(adopt).not.toHaveBeenCalled();
    expect(onEvent).toHaveBeenCalledTimes(1);
  });

  it("a failed attach starts disconnected and still passes held events on (AC-VEDDFD.1)", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    const onEvent = vi.fn();
    const adopt = vi.fn();
    await attachSession(
      (send) => {
        send(DISCONNECTED);
        return Promise.reject(new Error("backend down"));
      },
      onEvent,
      adopt,
    );
    expect(adopt).not.toHaveBeenCalled();
    expect(onEvent).toHaveBeenCalledTimes(1);
    expect(warn).toHaveBeenCalled();
    warn.mockRestore();
  });
});
