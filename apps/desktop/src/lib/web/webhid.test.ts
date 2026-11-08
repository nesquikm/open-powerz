/**
 * FR QFTCWZ, AC-QFTCWZ.3 (WebHID transport) and its AC-QFTCWZ.6 paths.
 *
 * Requests go out as one 64-byte output report (report ID 0, the frame zero-padded);
 * the reply comes back as 64-byte input reports, and a reply its headers declare longer
 * than 64 bytes is assembled from consecutive reports and trimmed to the declared length.
 */

import { describe, expect, it } from "vitest";
import { FakeHidDevice, toReports } from "./fakeDevices.test";
import { loadWasm } from "./testWasm.test";
import { openWebHid } from "./webhid";

type HidArg = Parameters<typeof openWebHid>[0];

function asHid(d: FakeHidDevice): HidArg {
  return d as unknown as HidArg;
}

/** A real multi-report reply: an ADC queue poll of the demo meter after 500 ms at 50 SPS. */
async function longQueueReply(): Promise<{ request: Uint8Array; reply: Uint8Array }> {
  const w = await loadWasm();
  const m = new w.DemoMeter("load-5v-1a");
  w.decodeAccept(m.transact(w.connectRequest(1)));
  const uid = w.decodeMemoryRead(
    m.transact(w.memoryReadRequest(2, 0x4001_0450, 12)),
    0x4001_0450,
    12,
  );
  const now = 1_790_000_000_000n;
  w.decodeAuthReply(m.transact(w.authRequest(3, 1, uid, now, 9n)), uid, now, 9n);
  w.decodeAccept(m.transact(w.startQueueRequest(4, 2)));
  m.advance(500);
  const request = w.getDataRequest(5, 0x002);
  return { request, reply: m.transact(request) };
}

describe("WebHID transport (AC-QFTCWZ.3)", () => {
  it("opens the device and sends the frame as one zero-padded 64-byte report with ID 0", async () => {
    const w = await loadWasm();
    const dev = new FakeHidDevice(() => toReports(new Uint8Array([0x05, 0x01, 0x00, 0x00])));
    const t = await openWebHid(asHid(dev));
    expect(dev.opened).toBe(true);
    const reply = await t.transact(w.connectRequest(1), 500);
    expect(dev.sent).toHaveLength(1);
    expect(dev.sent[0]?.reportId).toBe(0);
    const data = dev.sent[0]?.data ?? new Uint8Array();
    expect(data.length).toBe(64);
    expect(Array.from(data.slice(0, 4))).toEqual(Array.from(w.connectRequest(1)));
    expect(data.slice(4).every((b) => b === 0)).toBe(true);
    expect(() => w.decodeAccept(reply)).not.toThrow();
  });

  it("assembles a reply over 64 bytes from consecutive reports", async () => {
    const w = await loadWasm();
    const { request, reply } = await longQueueReply();
    expect(reply.length).toBeGreaterThan(128);
    const dev = new FakeHidDevice(() => toReports(reply));
    const t = await openWebHid(asHid(dev));
    const got = await t.transact(request, 1000);
    expect(got.length).toBe(reply.length);
    expect(Array.from(got)).toEqual(Array.from(reply));
    expect(w.decodeQueue(got, 2).length).toBe(w.decodeQueue(reply, 2).length);
  });

  it("carries every command, not only ADC reads", async () => {
    const w = await loadWasm();
    const dev = new FakeHidDevice(() => toReports(new Uint8Array([0x05, 0x02, 0x02, 0x00])));
    const t = await openWebHid(asHid(dev));
    await t.transact(w.startQueueRequest(2, 2), 500);
    expect(Array.from(dev.sent[0]?.data.slice(0, 4) ?? [])).toEqual(
      Array.from(w.startQueueRequest(2, 2)),
    );
  });

  it("a reply that never comes is a timeout", async () => {
    const dev = new FakeHidDevice(() => []);
    const t = await openWebHid(asHid(dev));
    await expect(t.transact(new Uint8Array([0x02, 0x01, 0x00, 0x00]), 40)).rejects.toMatchObject({
      kind: "timeout",
    });
  });

  it("a reply cut short after its first report is a timeout", async () => {
    const { request, reply } = await longQueueReply();
    const dev = new FakeHidDevice(() => toReports(reply).slice(0, 1));
    const t = await openWebHid(asHid(dev));
    await expect(t.transact(request, 60)).rejects.toMatchObject({ kind: "timeout" });
  });

  it("a failed sendReport is disconnected", async () => {
    const dev = new FakeHidDevice();
    dev.sendReport = () =>
      Promise.reject(new DOMException("Failed to write the report.", "NotAllowedError"));
    const t = await openWebHid(asHid(dev));
    await expect(t.transact(new Uint8Array([0x02, 0x01, 0x00, 0x00]), 500)).rejects.toMatchObject({
      kind: "disconnected",
    });
  });

  it("close() closes the device and later transfers are disconnected", async () => {
    const dev = new FakeHidDevice(() => toReports(new Uint8Array([0x05, 0x01, 0x00, 0x00])));
    const t = await openWebHid(asHid(dev));
    await t.close();
    expect(dev.opened).toBe(false);
    await expect(t.transact(new Uint8Array([0x02, 0x01, 0x00, 0x00]), 500)).rejects.toMatchObject({
      kind: "disconnected",
    });
  });
});
