/**
 * FR VG37JA: the WebUSB transport follows the native bulk transport.
 *
 * - AC-VG37JA.1: open discards every IN transfer already waiting (each drain read waits
 *   50 ms, until one times out) before the first request goes out.
 * - AC-VG37JA.2 (transport half): `recover()` clears the halt on the bulk OUT and IN
 *   endpoints, then discards stale IN data.
 * - AC-VG37JA.4: a transfer that ends with status "stall" clears the halt on that
 *   endpoint and fails as `timeout`; it never resolves as an empty reply.
 * - AC-VG37JA.5: the opt-in debug log (query parameter `debug`, or localStorage key
 *   `open-powerz.debug` set to "1"): every frame with its direction, length and first
 *   8 bytes in hex, plus open, drain, recovery and close; nothing without the flag.
 */

import { afterEach, describe, expect, it, vi } from "vitest";
import { FakeUsbDevice } from "./fakeDevices.test";
import { loadWasm } from "./testWasm.test";
import type { WebTransport } from "./transport";
import * as webusb from "./webusb";
import { openWebUsb } from "./webusb";

type UsbArg = Parameters<typeof openWebUsb>[0];
type UsbOptions = NonNullable<Parameters<typeof openWebUsb>[1]>;
/** The transport seam with the recover step this FR adds. */
type Recoverable = WebTransport & { recover?: () => Promise<void> };

const asUsb = (d: FakeUsbDevice): UsbArg => d as unknown as UsbArg;
const opts = (o: Record<string, unknown>): UsbOptions => o as unknown as UsbOptions;

const STALE_A = new Uint8Array([0x03, 0x2a, 0x00, 0x00]);
const STALE_B = new Uint8Array([0x03, 0x2b, 0x00, 0x00]);

/** An ACCEPT echoing the request's id. */
const accept = (frame: Uint8Array) => new Uint8Array([0x05, frame[1] ?? 0, 0x00, 0x00]);

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("drain at open (AC-VG37JA.1)", () => {
  it("discards every stale IN transfer before the first request is sent", async () => {
    const w = await loadWasm();
    const dev = new FakeUsbDevice(accept);
    dev.queueIn(STALE_A);
    dev.queueIn(STALE_B);
    const t = await openWebUsb(asUsb(dev), opts({ debug: false }));
    // Both stale frames were read at open, and nothing was written yet.
    expect(dev.pendingIn).toBe(0);
    expect(dev.outTransfers).toHaveLength(0);
    expect(dev.inResults.slice(0, 2)).toEqual([STALE_A, STALE_B]);
    const firstIn = dev.calls.indexOf("transferIn:6");
    expect(firstIn).toBeGreaterThan(dev.calls.indexOf("claimInterface:4"));
    // The first request gets its own reply, not the stale DISCONNECT.
    const reply = await t.transact(w.connectRequest(1), 500);
    expect(Array.from(reply)).toEqual([0x05, 0x01, 0x00, 0x00]);
    expect(dev.calls.indexOf("transferOut:5")).toBeGreaterThan(firstIn);
    await t.close();
  });

  it("reads with a 50 ms wait until one read times out", async () => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "Date"] });
    const dev = new FakeUsbDevice(accept);
    dev.queueIn(STALE_A);
    let opened = false;
    const opening = openWebUsb(asUsb(dev), opts({ debug: false })).then((t) => {
      opened = true;
      return t;
    });
    await vi.advanceTimersByTimeAsync(40);
    // The stale frame is gone at once; the read after it is still waiting.
    expect(dev.inResults).toEqual([STALE_A]);
    expect(dev.inTransfers.length).toBeGreaterThanOrEqual(2);
    expect(opened).toBe(false);
    await vi.advanceTimersByTimeAsync(20);
    expect(opened).toBe(true);
    expect(dev.outTransfers).toHaveLength(0);
    await (await opening).close();
  });
});

describe("transport recovery (AC-VG37JA.2)", () => {
  it("recover() clears the halt on bulk OUT and IN, then discards stale IN data", async () => {
    const w = await loadWasm();
    const dev = new FakeUsbDevice(accept);
    const t = (await openWebUsb(asUsb(dev), opts({ debug: false }))) as Recoverable;
    expect(typeof t.recover).toBe("function");
    const mark = dev.calls.length;
    dev.queueIn(STALE_A);
    await t.recover?.();
    const after = dev.calls.slice(mark);
    const out = after.indexOf("clearHalt:out:5");
    const inn = after.indexOf("clearHalt:in:6");
    expect(out).toBeGreaterThanOrEqual(0);
    expect(inn).toBeGreaterThan(out);
    expect(dev.pendingIn).toBe(0);
    expect(dev.inResults).toContainEqual(STALE_A);
    const reply = await t.transact(w.connectRequest(2), 500);
    expect(Array.from(reply)).toEqual([0x05, 0x02, 0x00, 0x00]);
  });

  it("the late reply of a timed-out request is discarded by recover()", async () => {
    const w = await loadWasm();
    const dev = new FakeUsbDevice(() => null);
    const t = (await openWebUsb(asUsb(dev), opts({ debug: false }))) as Recoverable;
    await expect(t.transact(w.connectRequest(1), 30)).rejects.toMatchObject({ kind: "timeout" });
    // The meter answers the first CONNECT after it timed out.
    dev.queueIn(new Uint8Array([0x05, 0x01, 0x00, 0x00]));
    await t.recover?.();
    dev.respond = accept;
    const reply = await t.transact(w.connectRequest(2), 500);
    expect(reply[1]).toBe(0x02);
  });
});

describe("stall handling (AC-VG37JA.4)", () => {
  it("an IN stall clears the halt on bulk IN and fails as a timeout, not an empty reply", async () => {
    const w = await loadWasm();
    const dev = new FakeUsbDevice(() => "stall");
    const t = await openWebUsb(asUsb(dev), opts({ debug: false }));
    const started = Date.now();
    await expect(t.transact(w.connectRequest(1), 2_000)).rejects.toMatchObject({
      kind: "timeout",
    });
    expect(Date.now() - started).toBeLessThan(1_000);
    expect(dev.calls).toContain("clearHalt:in:6");
    // The pipe still works afterwards (connect recovery can run).
    dev.respond = accept;
    const reply = await t.transact(w.connectRequest(2), 500);
    expect(Array.from(reply)).toEqual([0x05, 0x02, 0x00, 0x00]);
  });

  it("an OUT stall clears the halt on bulk OUT and fails as a timeout", async () => {
    const w = await loadWasm();
    const dev = new FakeUsbDevice(accept);
    const t = await openWebUsb(asUsb(dev), opts({ debug: false }));
    dev.stallNextOut = true;
    const started = Date.now();
    await expect(t.transact(w.connectRequest(1), 2_000)).rejects.toMatchObject({
      kind: "timeout",
    });
    expect(Date.now() - started).toBeLessThan(1_000);
    expect(dev.calls).toContain("clearHalt:out:5");
    const reply = await t.transact(w.connectRequest(2), 500);
    expect(reply[1]).toBe(0x02);
  });

  it("read() of a stalled IN transfer fails as a timeout", async () => {
    const dev = new FakeUsbDevice(() => null);
    const t = await openWebUsb(asUsb(dev), opts({ debug: false }));
    dev.queueIn("stall");
    await expect(t.read(2_000)).rejects.toMatchObject({ kind: "timeout" });
    expect(dev.calls).toContain("clearHalt:in:6");
  });
});

// ---- AC-VG37JA.5: the debug log ----

const SENT = new Uint8Array([
  0x0c, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b,
]);
const RECEIVED = new Uint8Array([0x41, 0x01, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff, 0x11, 0x22]);

/** Every console line written while the spies are on. */
function captureConsole(): string[] {
  const lines: string[] = [];
  for (const m of ["log", "debug", "info", "warn"] as const) {
    vi.spyOn(console, m).mockImplementation((...args: unknown[]) => {
      lines.push(args.map((a) => (typeof a === "string" ? a : JSON.stringify(a))).join(" "));
    });
  }
  return lines;
}

/** `line` lowercased with hex separators and 0x prefixes removed. */
const squash = (line: string) =>
  line
    .toLowerCase()
    .replace(/\b0x/g, "")
    .replace(/[\s:,]/g, "");

/** Open, one exchange, a recovery and a close. */
async function session(options: UsbOptions | undefined): Promise<void> {
  const dev = new FakeUsbDevice(() => RECEIVED);
  dev.queueIn(STALE_A);
  const t = (await openWebUsb(asUsb(dev), options)) as Recoverable;
  await t.transact(SENT, 500);
  await t.recover?.();
  await t.close();
}

describe("debug log (AC-VG37JA.5)", () => {
  it("the flag comes from the `debug` query parameter or localStorage", () => {
    const enabled = (webusb as unknown as Record<string, unknown>).webDebugEnabled as (
      search: string,
      storage: { getItem(k: string): string | null } | null,
    ) => boolean;
    expect(typeof enabled).toBe("function");
    expect((webusb as unknown as Record<string, unknown>).WEB_DEBUG_STORAGE_KEY).toBe(
      "open-powerz.debug",
    );
    const store = (v: string | null) => ({
      getItem: (k: string) => (k === "open-powerz.debug" ? v : null),
    });
    expect(enabled("?debug", null)).toBe(true);
    expect(enabled("?debug=1", null)).toBe(true);
    expect(enabled("?x=1&debug=1", store(null))).toBe(true);
    expect(enabled("", store("1"))).toBe(true);
    expect(enabled("", store(null))).toBe(false);
    expect(enabled("?other=1", null)).toBe(false);
  });

  it("with the flag, logs every frame (direction, length, first 8 bytes) and the lifecycle", async () => {
    const lines = captureConsole();
    await session(opts({ debug: true }));
    const sent = lines.find((l) => squash(l).includes("0c01020304050607"));
    expect(sent, lines.join("\n")).toBeDefined();
    expect(squash(sent ?? "")).not.toContain("0c0102030405060708");
    expect(sent).toMatch(/\bout\b/i);
    expect(sent).toMatch(/\b12\b/);
    const received = lines.find((l) => squash(l).includes("4101aabbccddeeff"));
    expect(received, lines.join("\n")).toBeDefined();
    expect(squash(received ?? "")).not.toContain("4101aabbccddeeff11");
    expect(received).toMatch(/\bin\b/i);
    expect(received).toMatch(/\b10\b/);
    // The stale frame drained at open is logged too.
    expect(lines.some((l) => squash(l).includes("032a0000"))).toBe(true);
    for (const event of [/open/i, /drain/i, /recover/i, /close/i]) {
      expect(
        lines.some((l) => event.test(l)),
        String(event),
      ).toBe(true);
    }
  });

  it("without the flag, logs nothing", async () => {
    const lines = captureConsole();
    await session(opts({ debug: false }));
    expect(lines).toEqual([]);
  });

  it("reads the flag from the page when no option is given", async () => {
    const lines = captureConsole();
    vi.stubGlobal("location", { search: "?debug=1" });
    vi.stubGlobal("localStorage", { getItem: () => null, setItem: () => {} });
    await session(undefined);
    expect(lines.some((l) => squash(l).includes("0c01020304050607"))).toBe(true);

    lines.length = 0;
    vi.stubGlobal("location", { search: "" });
    vi.stubGlobal("localStorage", {
      getItem: (k: string) => (k === "open-powerz.debug" ? "1" : null),
      setItem: () => {},
    });
    await session(undefined);
    expect(lines.some((l) => squash(l).includes("0c01020304050607"))).toBe(true);

    lines.length = 0;
    vi.stubGlobal("location", { search: "" });
    vi.stubGlobal("localStorage", { getItem: () => null, setItem: () => {} });
    await session(undefined);
    expect(lines).toEqual([]);
  });
});
