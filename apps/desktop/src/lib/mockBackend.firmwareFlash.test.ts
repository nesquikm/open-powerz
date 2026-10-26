/**
 * FR V9SK55 (AC .2–.6): the mock backend plays a firmware update like the real one, so
 * the UI suite can drive it: a chosen file, each stage with the write progress, the new
 * version, a failure at each stage (`?mock=firmware-fail-<stage>`), a file for another
 * model or one that fails inspection, the typed confirmation re-checked, and the meter
 * held for the update (stream, PD capture, triggering and switching refused).
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { BackendEvent, FlashDone, FlashEvent, UiError } from "./backend";
import { MOCK_MODES, MockBackend, mockModeFromUrl, type MockMode } from "./mockBackend";

const DEMO = "demo:load-5v-1a";

const FAIL_MODES = [
  ["firmware-fail-switching", "switching"],
  ["firmware-fail-erasing", "erasing"],
  ["firmware-fail-writing", "writing"],
  ["firmware-fail-verifying", "verifying"],
  ["firmware-fail-returning", "returning"],
] as const;

/** Run a mock flash to its end on fake timers; returns the outcome or the error and
 * every stage reported. */
async function runFlash(
  backend: MockBackend,
  confirmation = "2.0.7",
  deviceId = DEMO,
): Promise<{ done: FlashDone | null; error: UiError | null; stages: FlashEvent[] }> {
  const path = await backend.firmwareChoose();
  if (path === null) throw new Error("the mock chose no file");
  const stages: FlashEvent[] = [];
  let done: FlashDone | null = null;
  let error: UiError | null = null;
  const p = backend
    .firmwareFlash(deviceId, path, confirmation, (s) => stages.push(s))
    .then(
      (d) => (done = d),
      (e: UiError) => (error = e),
    );
  await vi.runAllTimersAsync();
  await p;
  return { done, error, stages };
}

function names(stages: FlashEvent[]): string[] {
  const out: string[] = [];
  for (const s of stages) if (out.at(-1) !== s.stage) out.push(s.stage);
  return out;
}

describe("MockBackend firmware update", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("knows the firmware modes", () => {
    const modes: readonly string[] = MOCK_MODES;
    for (const [mode] of FAIL_MODES) {
      expect(modes).toContain(mode);
      expect(mockModeFromUrl(`?mock=${mode}`)).toBe(mode);
    }
    expect(modes).toContain("firmware-other-model");
    expect(modes).toContain("firmware-bad-file");
  });

  it("the open dialog picks a synthetic firmware file", async () => {
    const path = await new MockBackend().firmwareChoose();
    expect(path).toMatch(/\.mencrypt$/);
  });

  it("a successful update reports every stage and the new version", async () => {
    const { done, error, stages } = await runFlash(new MockBackend());
    expect(error).toBeNull();
    expect(done).toEqual({ outcome: "verified", version: "2.0.7" });
    // Like the backend, each jump is followed by a replug prompt (AC-V9SK55.4).
    expect(names(stages)).toEqual([
      "switching",
      "replug",
      "erasing",
      "writing",
      "verifying",
      "finished",
      "returning",
      "replug",
    ]);
    expect(stages.filter((s) => s.stage === "replug")).toEqual([
      { stage: "replug", to: "bootloader", reply: "none" },
      { stage: "replug", to: "application", reply: "none" },
    ]);
    const writes = stages.flatMap((s) => (s.stage === "writing" ? [s] : []));
    expect(writes.length).toBeGreaterThanOrEqual(3);
    const total = writes[0]!.total;
    expect(total).toBeGreaterThan(0);
    expect(writes.every((w) => w.total === total)).toBe(true);
    for (let i = 1; i < writes.length; i++) {
      expect(writes[i]!.done).toBeGreaterThanOrEqual(writes[i - 1]!.done);
    }
    expect(writes.at(-1)!.done).toBe(total);
  });

  it("a meter in bootloader mode is updated without the switch", async () => {
    const { done, stages } = await runFlash(new MockBackend("bootloader"), "2.0.7", "usb:1:9");
    expect(done).toEqual({ outcome: "verified", version: "2.0.7" });
    expect(names(stages)[0]).toBe("erasing");
  });

  it("re-checks the typed confirmation and sends nothing when it is not the version", async () => {
    const backend = new MockBackend();
    for (const typed of ["", "2.0.6", "2.0.7x"]) {
      const { done, error, stages } = await runFlash(backend, typed);
      expect(done).toBeNull();
      expect(error?.kind).toBe("usage");
      expect(error?.message).toContain("2.0.7");
      expect(stages).toEqual([]);
    }
    expect(backend.firmwareFlashLog()).toEqual([]);
  });

  it("records each update it ran", async () => {
    const backend = new MockBackend();
    await runFlash(backend);
    expect(backend.firmwareFlashLog()).toEqual([
      { deviceId: DEMO, path: expect.stringMatching(/\.mencrypt$/), confirmation: "2.0.7" },
    ]);
  });

  for (const [mode, stage] of FAIL_MODES) {
    it(`?mock=${mode} fails at the ${stage} stage with the recovery steps`, async () => {
      const { done, error, stages } = await runFlash(new MockBackend(mode as MockMode));
      expect(done).toBeNull();
      expect(error).not.toBeNull();
      expect(error!.kind).not.toBe("usage");
      expect(error!.message.length).toBeGreaterThan(0);
      expect(error!.hint ?? "").toContain("Back");
      expect(error!.hint ?? "").toMatch(/plug/i);
      expect(error!.hint ?? "").toMatch(/again|retry/i);
      expect(stages.at(-1)?.stage).toBe(stage);
    });
  }

  it("?mock=firmware-other-model inspects as a KM002C file", async () => {
    const backend = new MockBackend("firmware-other-model");
    const path = (await backend.firmwareChoose()) ?? "";
    const info = await backend.firmwareInspect(path);
    expect(info.model).toBe("KM002C");
  });

  it("?mock=firmware-bad-file fails inspection with a usage error", async () => {
    const backend = new MockBackend("firmware-bad-file");
    const path = (await backend.firmwareChoose()) ?? "";
    await expect(backend.firmwareInspect(path)).rejects.toMatchObject({ kind: "usage" });
  });

  it("holds the meter during the update: stream, PD, trigger and switching are refused", async () => {
    const backend = new MockBackend();
    const events: BackendEvent[] = [];
    await backend.connect(DEMO, 50, (e) => events.push(e));
    const path = (await backend.firmwareChoose()) ?? "";
    let settled = false;
    const flashing = backend
      .firmwareFlash(DEMO, path, "2.0.7", () => {})
      .finally(() => {
        settled = true;
      });
    await vi.advanceTimersByTimeAsync(1);
    expect(settled).toBe(false);
    // The update took the meter from the stream.
    expect(events.some((e) => e.type === "status" && e.data.state === "disconnected")).toBe(true);
    const refusals = [
      backend.connect(DEMO, 50, () => {}),
      backend.connect("demo:idle", 50, () => {}),
      backend.pdStart(),
      backend.triggerOpen(),
      backend.firmwareFlash(DEMO, path, "2.0.7", () => {}),
    ];
    for (const r of refusals) {
      await expect(r).rejects.toMatchObject({
        kind: "usage",
        message: expect.stringMatching(/firmware/i),
      });
    }
    await vi.runAllTimersAsync();
    await flashing;
    // Released afterwards.
    await expect(backend.connect(DEMO, 50, () => {})).resolves.toBeUndefined();
    await backend.disconnect();
  });
});
