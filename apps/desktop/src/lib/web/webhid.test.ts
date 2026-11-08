/**
 * FR QFTCWZ, AC-QFTCWZ.3 (WebHID transport) and its AC-QFTCWZ.6 paths.
 *
 * The WebHID fallback is ADC-snapshot only (docs/protocol.md "HID limits", [device]
 * firmware 2.0.6: any reply over 64 bytes hangs and reboots the meter, control
 * commands get no reply). The transport's guard lets through GET_DATA with attribute
 * mask 0x001 and nothing else: every other frame is refused with a `usage` error
 * before `sendReport` is called. A request goes out as one 64-byte output report
 * (report ID 0, the frame zero-padded), and each 64-byte input report is one reply:
 * there is no multi-report assembly.
 */

import { describe, expect, it } from "vitest";
import { FakeHidDevice, toReports } from "./fakeDevices.test";
import { SAFE_BLOCKS, loadWasm } from "./testWasm.test";
import { openWebHid } from "./webhid";

type HidArg = Parameters<typeof openWebHid>[0];

function asHid(d: FakeHidDevice): HidArg {
  return d as unknown as HidArg;
}

/** A real reply that declares more than 64 bytes: an ADC queue poll of the demo meter
 * after 500 ms at 50 SPS. */
async function longQueueReply(): Promise<Uint8Array> {
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
  return m.transact(w.getDataRequest(5, 0x002));
}

/** Every frame the read-only session can build except GET_DATA 0x001, plus raw bytes. */
async function refusedFrames(): Promise<[string, Uint8Array][]> {
  const w = await loadWasm();
  const adc = w.getDataRequest(9, 0x001);
  const padded = new Uint8Array(adc.length + 4);
  padded.set(adc);
  padded.set([1, 2, 3, 4], adc.length);
  const frames: [string, Uint8Array][] = [
    ["connect", w.connectRequest(1)],
    ["auth", w.authRequest(2, 1, new Uint8Array(12), 1_790_000_000_000n, 9n)],
    ["stop_queue", w.stopQueueRequest(3)],
    ["start_pd", w.startPdSnifferRequest(4)],
    ["stop_pd", w.stopPdSnifferRequest(5)],
    ["pd_poll", w.pdPollRequest(6, false)],
    ["pd_poll_adc", w.pdPollRequest(7, true)],
    ["get_data:adc_queue", w.getDataRequest(8, 0x002)],
    ["get_data:adc+queue", w.getDataRequest(8, 0x003)],
    ["get_data:settings", w.getDataRequest(8, 0x008)],
    ["get_data:pd_packet", w.getDataRequest(8, 0x010)],
    ["get_data:pd_status", w.getDataRequest(8, 0x020)],
    ["get_data:adc+pd", w.getDataRequest(8, 0x011)],
    ["get_data:adc with trailing bytes", padded],
    ["set_data", new Uint8Array([0x48, 0x01, 0x00, 0x00])],
    ["empty", new Uint8Array(0)],
  ];
  for (let code = 0; code <= 3; code++) {
    frames.push([`start_queue:${code}`, w.startQueueRequest(10 + code, code)]);
  }
  for (const [addr, len] of SAFE_BLOCKS) {
    frames.push([`memory_read:${addr.toString(16)}`, w.memoryReadRequest(20, addr, len)]);
  }
  return frames;
}

describe("WebHID transport (AC-QFTCWZ.3)", () => {
  it("sends GET_DATA 0x001 as one zero-padded 64-byte report with ID 0", async () => {
    const w = await loadWasm();
    const meter = new w.DemoMeter("load-5v-1a");
    const dev = new FakeHidDevice((report) => toReports(meter.transact(report.slice(0, 4))));
    const t = await openWebHid(asHid(dev));
    expect(dev.opened).toBe(true);
    const request = w.getDataRequest(1, 0x001);
    const reply = await t.transact(request, 500);
    expect(dev.sent).toHaveLength(1);
    expect(dev.sent[0]?.reportId).toBe(0);
    const data = dev.sent[0]?.data ?? new Uint8Array();
    expect(data.length).toBe(64);
    expect(Array.from(data.slice(0, request.length))).toEqual(Array.from(request));
    expect(data.slice(request.length).every((b) => b === 0)).toBe(true);
    expect(w.decodeAdc(reply).vbus_v).toBeCloseTo(5, 1);
  });

  it("lets GET_DATA 0x001 through whatever its transaction id", async () => {
    const w = await loadWasm();
    const meter = new w.DemoMeter("idle");
    const dev = new FakeHidDevice((report) => toReports(meter.transact(report.slice(0, 4))));
    const t = await openWebHid(asHid(dev));
    for (const id of [0, 1, 2, 127, 200, 255]) {
      await t.transact(w.getDataRequest(id, 0x001), 500);
    }
    expect(dev.sent).toHaveLength(6);
  });

  it("refuses every other frame with a usage error before anything is sent", async () => {
    const dev = new FakeHidDevice(() => toReports(new Uint8Array([0x05, 0x01, 0x00, 0x00])));
    const t = await openWebHid(asHid(dev));
    for (const [name, frame] of await refusedFrames()) {
      await expect(t.transact(frame, 200), name).rejects.toMatchObject({ kind: "usage" });
      expect(dev.sent, `${name} reached sendReport`).toHaveLength(0);
    }
  });

  it("a refused frame leaves the transport usable for ADC snapshots", async () => {
    const w = await loadWasm();
    const meter = new w.DemoMeter("load-5v-1a");
    const dev = new FakeHidDevice((report) => toReports(meter.transact(report.slice(0, 4))));
    const t = await openWebHid(asHid(dev));
    await expect(t.transact(w.connectRequest(1), 200)).rejects.toMatchObject({ kind: "usage" });
    const reply = await t.transact(w.getDataRequest(2, 0x001), 500);
    expect(w.decodeAdc(reply).ibus_a).toBeCloseTo(1, 1);
    expect(dev.sent).toHaveLength(1);
  });

  it("takes one 64-byte input report as the whole reply, even when its headers declare more", async () => {
    const w = await loadWasm();
    const long = await longQueueReply();
    expect(long.length).toBeGreaterThan(128);
    const reports = toReports(long);
    const dev = new FakeHidDevice(() => reports);
    const t = await openWebHid(asHid(dev));
    // A short timeout: an assembling transport would wait for the later reports.
    const got = await t.transact(w.getDataRequest(1, 0x001), 500);
    expect(Array.from(got)).toEqual(Array.from(reports[0]!.slice(0, got.length)));
    expect(got.length).toBe(64);
  });

  it("each later input report is its own reply to read(), not a continuation", async () => {
    const w = await loadWasm();
    const first = new Uint8Array(64).fill(0x11);
    const second = new Uint8Array(64).fill(0x22);
    first[0] = 0x41;
    second[0] = 0x41;
    const dev = new FakeHidDevice(() => [first, second]);
    const t = await openWebHid(asHid(dev));
    const a = await t.transact(w.getDataRequest(1, 0x001), 500);
    const b = await t.read(500);
    expect(Array.from(a)).toEqual(Array.from(first));
    expect(Array.from(b)).toEqual(Array.from(second));
  });

  it("a reply that never comes is a timeout", async () => {
    const w = await loadWasm();
    const dev = new FakeHidDevice(() => []);
    const t = await openWebHid(asHid(dev));
    await expect(t.transact(w.getDataRequest(1, 0x001), 40)).rejects.toMatchObject({
      kind: "timeout",
    });
  });

  it("a failed sendReport is disconnected", async () => {
    const w = await loadWasm();
    const dev = new FakeHidDevice();
    dev.sendReport = () =>
      Promise.reject(new DOMException("Failed to write the report.", "NotAllowedError"));
    const t = await openWebHid(asHid(dev));
    await expect(t.transact(w.getDataRequest(1, 0x001), 500)).rejects.toMatchObject({
      kind: "disconnected",
    });
  });

  it("a device that cannot be opened is device_busy", async () => {
    const dev = new FakeHidDevice();
    dev.open = () =>
      Promise.reject(new DOMException("Failed to open the device.", "NotAllowedError"));
    await expect(openWebHid(asHid(dev))).rejects.toMatchObject({ kind: "device_busy" });
  });

  it("close() closes the device and later transfers are disconnected", async () => {
    const w = await loadWasm();
    const dev = new FakeHidDevice(() => toReports(new Uint8Array([0x41, 0x01, 0x00, 0x00])));
    const t = await openWebHid(asHid(dev));
    await t.close();
    expect(dev.opened).toBe(false);
    await expect(t.transact(w.getDataRequest(1, 0x001), 500)).rejects.toMatchObject({
      kind: "disconnected",
    });
  });
});
