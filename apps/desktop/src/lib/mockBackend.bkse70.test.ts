/**
 * FR BKSE70 (AC .5) in the mock backend: like the real one, an update of the meter
 * listed on the old bootloader (`?mock=legacy-bootloader`) ends at once with the
 * old-bootloader message and reports no stage.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { FlashEvent, UiError } from "./backend";
import { MockBackend } from "./mockBackend";

/** The how-to section the message links, written out independently. */
const HOWTO_URL =
  "https://github.com/nesquikm/open-powerz/blob/main/docs/how-to/update-firmware.md#old-bootloader";

async function flashOldBootloader(): Promise<{ error: UiError | null; stages: FlashEvent[] }> {
  const backend = new MockBackend("legacy-bootloader");
  const meter = (await backend.listDevices()).find((d) => d.mode === "legacy_bootloader");
  if (meter === undefined) throw new Error("the mock lists no old-bootloader meter");
  const path = await backend.firmwareChoose();
  if (path === null) throw new Error("the mock chose no file");
  const stages: FlashEvent[] = [];
  let error: UiError | null = null;
  let done = false;
  const p = backend
    .firmwareFlash(meter.id, path, "2.0.7", (s) => stages.push(s))
    .then(
      () => (done = true),
      (e: UiError) => (error = e),
    );
  await vi.runAllTimersAsync();
  await p;
  expect(done, "the update of an old bootloader must not succeed").toBe(false);
  return { error, stages };
}

describe("MockBackend update on the old bootloader (AC-BKSE70.5)", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("ends with the old-bootloader message", async () => {
    const { error } = await flashOldBootloader();
    const message = (error as UiError | null)?.message ?? "";
    expect(message.toLowerCase()).toContain("supported only experimentally, from the cli");
    expect(message).toContain(HOWTO_URL);
  });

  it("reports no stage: nothing reaches the meter", async () => {
    const { stages } = await flashOldBootloader();
    expect(stages).toEqual([]);
  });
});
