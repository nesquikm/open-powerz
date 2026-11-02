/**
 * AC-AXHK2M.17 on the page side: a listed meter whose mode changes under the same
 * entry (same id) is shown in the new mode by the open list and the closed device
 * select alike: "(bootloader mode)" while it is in its bootloader, gone once it is back.
 *
 * The device select renders on the server side here, so a test reads its HTML. The
 * mock backend's `mode-flip` mode plays the change for the Playwright suite.
 */

import type { Component } from "svelte";
import { render } from "svelte/server";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { DeviceListing } from "./backend";
import DevicePicker from "./components/DevicePicker.svelte";
import { deviceOptionLabel } from "./format";
import { MOCK_MODE_FLIP, MOCK_MODES, MockBackend, mockModeFromUrl } from "./mockBackend";
import { Session } from "./session.svelte";

const SUFFIX = "(bootloader mode)";

function meter(mode: "application" | "bootloader"): DeviceListing {
  return {
    id: "usb:1:7",
    label: mode === "bootloader" ? `KM003C — FLIP-0001 ${SUFFIX}` : "KM003C — FLIP-0001",
    model: "KM003C",
    serial: "FLIP-0001",
    bus: "1",
    address: 7,
    demo_scenario: null,
    mode,
  };
}

const DEMO: DeviceListing = {
  id: "demo:idle",
  label: "Demo device — idle",
  model: "KM003C",
  serial: "123456",
  bus: null,
  address: null,
  demo_scenario: "idle",
  mode: "application",
};

/** The device select's HTML for `session`, without Svelte's comment markers. */
function picker(session: Session): string {
  const props = { session, hideSerial: false };
  const body = render(DevicePicker as Component<Record<string, unknown>>, { props }).body;
  return body.replace(/<!--.*?-->/g, "");
}

/** The text of the option for `id` in `markup`. */
function optionText(markup: string, id: string): string {
  const re = new RegExp(`<option[^>]*value="${id.replace(/[.:]/g, "\\$&")}"[^>]*>([^<]*)</option>`);
  const m = re.exec(markup);
  if (!m) throw new Error(`no option ${id} in ${markup}`);
  return (m[1] ?? "").trim();
}

/** The first listed entry; fails the test when the list is empty. */
function first(s: Session): DeviceListing {
  const d = s.devices[0];
  if (d === undefined) throw new Error("no device listed");
  return d;
}

/** The closed select's tooltip: the picked entry's full label. */
function selectTitle(markup: string): string {
  const m = /<select[^>]*title="([^"]*)"/.exec(markup);
  return m?.[1] ?? "";
}

describe("a listed meter changing mode under the same entry (AC-AXHK2M.17)", () => {
  it("AC-AXHK2M.17: the session keeps the entry picked and follows its mode both ways", () => {
    const s = new Session();
    s.setDevices([meter("application"), DEMO]);
    s.select("usb:1:7");
    expect(deviceOptionLabel(first(s), false)).not.toContain(SUFFIX);

    s.setDevices([meter("bootloader"), DEMO]);
    expect(s.selectedId).toBe("usb:1:7");
    expect(s.devices.find((d) => d.id === "usb:1:7")?.mode).toBe("bootloader");
    expect(deviceOptionLabel(first(s), false)).toBe(`KM003C — FLIP-0001 ${SUFFIX}`);

    s.setDevices([meter("application"), DEMO]);
    expect(s.selectedId).toBe("usb:1:7");
    expect(deviceOptionLabel(first(s), false)).toBe("KM003C — FLIP-0001");
  });

  it("AC-AXHK2M.17: the open list and the closed select both say bootloader mode, then drop it", () => {
    const s = new Session();
    s.setDevices([meter("application"), DEMO]);
    s.select("usb:1:7");
    let markup = picker(s);
    expect(optionText(markup, "usb:1:7")).not.toContain(SUFFIX);
    expect(selectTitle(markup)).not.toContain(SUFFIX);

    s.setDevices([meter("bootloader"), DEMO]);
    markup = picker(s);
    expect(optionText(markup, "usb:1:7")).toContain(SUFFIX);
    expect(selectTitle(markup)).toContain(SUFFIX);

    s.setDevices([meter("application"), DEMO]);
    markup = picker(s);
    expect(optionText(markup, "usb:1:7")).not.toContain(SUFFIX);
    expect(selectTitle(markup)).not.toContain(SUFFIX);
  });
});

describe("MockBackend mode-flip (AC-AXHK2M.17)", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("AC-AXHK2M.17: is a mock mode chosen with ?mock=mode-flip", () => {
    expect(MOCK_MODES).toContain("mode-flip");
    expect(mockModeFromUrl("?mock=mode-flip")).toBe("mode-flip");
  });

  it("AC-AXHK2M.17: lists one meter in application mode, flips it to bootloader under the same id, then back", async () => {
    const backend = new MockBackend("mode-flip");
    const lists: DeviceListing[][] = [];
    await backend.watchDevices((l) => lists.push(l));
    const id = `usb:1:${MOCK_MODE_FLIP.address}`;
    const current = () => {
      const last = lists.at(-1) ?? [];
      return last.find((d) => d.demo_scenario === null);
    };

    expect(current()).toMatchObject({ id, mode: "application", serial: MOCK_MODE_FLIP.serial });
    await vi.advanceTimersByTimeAsync(MOCK_MODE_FLIP.appMs + 10);
    expect(current()).toMatchObject({ id, mode: "bootloader" });
    expect(current()?.label).toContain(SUFFIX);
    await vi.advanceTimersByTimeAsync(MOCK_MODE_FLIP.bootMs + 10);
    expect(current()).toMatchObject({ id, mode: "application" });
    expect(current()?.label).not.toContain(SUFFIX);
    // Demo entries stay listed throughout.
    expect(lists.at(-1)?.some((d) => d.id === "demo:idle")).toBe(true);
  });
});
