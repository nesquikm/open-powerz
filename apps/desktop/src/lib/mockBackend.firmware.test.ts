/**
 * The mock backend mirrors the backend's device mode, versions and firmware warnings
 * (FR V9SK52, AC .4 and .5).
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { BackendEvent } from "./backend";
import { MOCK_MODES, MockBackend, mockModeFromUrl } from "./mockBackend";

describe("MockBackend firmware details", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("lists every demo entry in application mode", async () => {
    const list = await new MockBackend().listDevices();
    expect(list.length).toBeGreaterThan(0);
    for (const d of list) expect(d.mode).toBe("application");
  });

  it("connected status carries the mode, both versions and the outdated warning", async () => {
    const backend = new MockBackend();
    const events: BackendEvent[] = [];
    await backend.connect("demo:load-5v-1a", 50, (e) => events.push(e));
    await backend.disconnect();
    expect(events[0]).toMatchObject({
      type: "status",
      data: {
        state: "connected",
        mode: "application",
        firmware_version: "2.0.6",
        bootloader_version: "2.5",
        warnings: ["outdated"],
      },
    });
  });

  it("connected status says the update check was read and nothing went wrong", async () => {
    const backend = new MockBackend();
    const events: BackendEvent[] = [];
    await backend.connect("demo:load-5v-1a", 50, (e) => events.push(e));
    await backend.disconnect();
    const first = events[0];
    expect(first?.type).toBe("status");
    if (first?.type !== "status" || first.data.state !== "connected") {
      throw new Error(`not a connected status: ${JSON.stringify(first)}`);
    }
    expect(first.data.update_required).toBe(false);
    expect(first.data.firmware_problems).toEqual([]);
  });

  it("`?mock=bootloader` is a mock mode", () => {
    expect(MOCK_MODES).toContain("bootloader");
    expect(mockModeFromUrl("?mock=bootloader")).toBe("bootloader");
  });

  it("`bootloader` lists a meter in bootloader mode before the demo entries", async () => {
    const list = await new MockBackend("bootloader").listDevices();
    const boot = list.filter((d) => d.mode === "bootloader");
    expect(boot).toHaveLength(1);
    expect(boot[0]?.id).toBe("usb:1:9");
    expect(boot[0]?.demo_scenario).toBeNull();
    expect(boot[0]?.label.toLowerCase()).toContain("bootloader");
    expect(list[0]?.id).toBe(boot[0]?.id);
    // The demo device is still there, in application mode.
    expect(list.filter((d) => d.demo_scenario !== null).length).toBe(4);
  });
});
