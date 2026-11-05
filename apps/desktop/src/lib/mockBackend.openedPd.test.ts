/** FR AXHK2K: the mock's opened PD file follows the backend's rule. Only a command the
 * page sends releases it (AC-AXHK2K.5, AC-AXHK2K.6); connecting any meter, an automatic
 * reconnect included, does not. Also the attach snapshot's PD state (AC-AXHK2K.7). */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { MOCK_REPLUG, MockBackend } from "./mockBackend";

const LOAD = "demo:load-5v-1a";
const PD = "demo:pd-9v-2a";

/** A mock holding an opened capture of the records a first meter captured. */
async function withOpenedFile(mode: "replug" | null = null): Promise<MockBackend> {
  const backend = new MockBackend(mode);
  await backend.connect(LOAD, 50, () => {});
  await backend.pdStart();
  await vi.advanceTimersByTimeAsync(2_000);
  await backend.pdStop();
  await backend.pdSave("capture.pdcap");
  await backend.pdOpen();
  return backend;
}

describe("The mock's opened PD file (AC-AXHK2K.6)", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("stays open while another meter connects", async () => {
    const backend = await withOpenedFile();
    await backend.connect(PD, 50, () => {});
    await expect(backend.pdDetails(0, true)).resolves.toMatchObject({ index: 0 });
  });

  it("stays open through an automatic reconnect", async () => {
    const backend = await withOpenedFile("replug");
    const meter = (await backend.listDevices()).find((d) => d.demo_scenario === null);
    if (!meter) throw new Error("no real meter listed");
    await backend.connect(meter.id, 50, () => {});
    await vi.advanceTimersByTimeAsync(MOCK_REPLUG.beforeMs + MOCK_REPLUG.awayMs + 500);
    expect(backend.connectedIds()).toHaveLength(2);
    await expect(backend.pdDetails(0, true)).resolves.toMatchObject({ index: 0 });
  });

  it("goes when the page releases it", async () => {
    const backend = await withOpenedFile();
    await backend.pdReleaseOpened();
    await expect(backend.pdDetails(0, true)).rejects.toMatchObject({
      message: "That PD record is not in the opened capture.",
    });
  });

  it("is released without a meter, and a second release changes nothing", async () => {
    const backend = await withOpenedFile();
    await backend.disconnect();
    await expect(backend.pdReleaseOpened()).resolves.toBeUndefined();
    await expect(backend.pdReleaseOpened()).resolves.toBeUndefined();
    await expect(backend.pdDetails(0, true)).rejects.toMatchObject({ kind: "usage" });
  });

  it("releasing it leaves the live capture alone", async () => {
    const backend = new MockBackend();
    await backend.connect(LOAD, 50, () => {});
    await backend.pdStart();
    await vi.advanceTimersByTimeAsync(2_000);
    await backend.pdReleaseOpened();
    await expect(backend.pdDetails(0)).resolves.toMatchObject({ index: 0 });
  });
});

describe("The attach snapshot's PD state (AC-AXHK2K.7)", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("says which meter captures", async () => {
    const backend = new MockBackend();
    await backend.connect(LOAD, 50, () => {});
    await backend.connect(PD, 50, () => {});
    await backend.meter(PD).pdStart();
    await expect(backend.attach(() => {}, PD)).resolves.toMatchObject({ pd_capturing: true });
    await expect(backend.attach(() => {}, LOAD)).resolves.toMatchObject({ pd_capturing: false });
    await backend.meter(PD).pdStop();
    await expect(backend.attach(() => {}, PD)).resolves.toMatchObject({ pd_capturing: false });
    await backend.disconnect();
    await backend.disconnect();
  });
});
