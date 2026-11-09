/**
 * FR CJ3QCD, AC-CJ3QCD.3: after a refused rate change the rate picker shows the rate the
 * session streams at. Both sample-rate pickers (the meter tile's MeterControls and the
 * TopBar) hand their change to `pickRate` in `src/lib/recording.svelte.ts`: it asks for
 * the picked rate, waits for the answer, then puts the select back on the rate in use
 * (a one-way `value={rateSps}` does not reset when the prop stays the same).
 *
 * No DOM: the select is a stand-in with a `value`, as the change handler passes it.
 */

import { readFileSync } from "node:fs";
import { describe, expect, it, vi } from "vitest";
import * as recording from "../recording.svelte";

type PickRate = (
  select: { value: string },
  onSelectRate: (sps: number) => void | Promise<void>,
  rateInUse: () => number,
) => Promise<void>;

const pickRate = (recording as unknown as { pickRate?: PickRate }).pickRate;

describe("the rate picker after a rate change (AC-CJ3QCD.3)", () => {
  it("cj3qcd: pickRate is exported for the pickers", () => {
    expect(typeof pickRate).toBe("function");
  });

  it("cj3qcd: a refused change puts the picker back on the rate the session streams at", async () => {
    const select = { value: "1000" };
    let streaming = 50;
    const onSelectRate = vi.fn(async (): Promise<void> => {
      // The backend refuses: the stream stays at 50 SPS.
      await Promise.resolve();
      streaming = 50;
    });
    await pickRate?.(select, onSelectRate, () => streaming);
    expect(onSelectRate).toHaveBeenCalledWith(1000);
    expect(select.value).toBe("50");
  });

  it("cj3qcd: a refusal the handler rejects with also puts the picker back", async () => {
    const select = { value: "1000" };
    const onSelectRate = vi.fn(async () => {
      throw new Error("The rate stays at 50 samples/s or less while the trigger panel is open.");
    });
    await pickRate?.(select, onSelectRate, () => 50).catch(() => {});
    expect(select.value).toBe("50");
  });

  it("cj3qcd: the resync waits for the answer, not just the click", async () => {
    const select = { value: "1000" };
    let streaming = 50;
    let answer: (() => void) | undefined;
    const onSelectRate = vi.fn(
      () =>
        new Promise<void>((resolve) => {
          answer = () => {
            streaming = 1000;
            resolve();
          };
        }),
    );
    const done = pickRate?.(select, onSelectRate, () => streaming);
    answer?.();
    await done;
    expect(select.value).toBe("1000");
  });

  it("cj3qcd: an accepted change keeps the picked rate on show", async () => {
    const select = { value: "1000" };
    let streaming = 50;
    const onSelectRate = vi.fn(async (sps: number) => {
      streaming = sps;
    });
    await pickRate?.(select, onSelectRate, () => streaming);
    expect(select.value).toBe("1000");
  });

  it("cj3qcd: both sample-rate pickers hand their change to pickRate", () => {
    for (const file of ["MeterControls.svelte", "TopBar.svelte"]) {
      const src = readFileSync(new URL(`./${file}`, import.meta.url), "utf8");
      const select = /<select\s+aria-label="Sample rate"[\s\S]*?\{#each/.exec(src)?.[0] ?? "";
      expect(select, file).toMatch(/onchange=\{[^}]*pickRate\(/);
    }
  });
});
