/**
 * FR TGE83C: the mock backend's PD capture, which the Playwright suite drives. Like the
 * simulator's PD negotiation scenario it captures an attach, an SPR negotiation, cable
 * (SOP') traffic and a Hard Reset, with a GoodCRC after every message, and it keeps
 * capturing for as long as the capture runs. Rows go out batched at most 60 times a
 * second; details come on demand with byte ranges for every field.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { BackendEvent, PdField, PdRow } from "./backend";
import { MockBackend, pdSyntheticFromUrl } from "./mockBackend";

function pdRows(events: BackendEvent[]): PdRow[] {
  return events.flatMap((e) => (e.type === "pd_rows" ? e.data.rows : []));
}

function pdBatches(events: BackendEvent[]): number {
  return events.filter((e) => e.type === "pd_rows").length;
}

function find(nodes: PdField[], name: string): PdField {
  const n = nodes.find((f) => f.name === name);
  if (!n) throw new Error(`no field ${name} in ${nodes.map((f) => f.name).join(", ")}`);
  return n;
}

async function capturing(ms: number): Promise<{ backend: MockBackend; events: BackendEvent[] }> {
  const backend = new MockBackend();
  const events: BackendEvent[] = [];
  await backend.connect("demo:load-5v-1a", 50, (e) => events.push(e));
  await backend.pdStart();
  vi.advanceTimersByTime(ms);
  return { backend, events };
}

describe("MockBackend PD capture (AC-TGE83C.1, AC-TGE83C.2)", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("refuses to start without a session", async () => {
    await expect(new MockBackend().pdStart()).rejects.toMatchObject({ kind: "usage" });
  });

  it("reports the capture state on start and stop", async () => {
    const { backend, events } = await capturing(100);
    await backend.pdStop();
    const states = events.flatMap((e) => (e.type === "pd_state" ? [e.data.capturing] : []));
    expect(states).toEqual([true, false]);
    await backend.disconnect();
  });

  it("plays the demo PD negotiation with GoodCRCs, cable traffic and events", async () => {
    const { backend, events } = await capturing(5_000);
    await backend.disconnect();
    const rows = pdRows(events);
    const names = rows.map((r) => r.name);
    expect(names).toContain("Source_Capabilities");
    expect(names).toContain("Request");
    expect(names).toContain("Accept");
    expect(names).toContain("PS_RDY");
    expect(names).toContain("GoodCRC");
    expect(rows.some((r) => r.kind === "event" && /attach/i.test(r.name))).toBe(true);
    expect(rows.some((r) => r.kind === "event" && /hard.?reset/i.test(r.name))).toBe(true);
    expect(rows.some((r) => r.kind === "message" && r.sop === "SOP'")).toBe(true);
    expect(rows.map((r) => r.index)).toEqual(rows.map((_, i) => i));
    const caps = rows.find((r) => r.name === "Source_Capabilities")!;
    expect(caps).toMatchObject({ kind: "message", sop: "SOP", direction: "source_to_sink" });
    expect(caps.objects).toBeGreaterThan(0);
    expect(caps.summary).toMatch(/^Source_Capabilities/);
  });

  it("keeps capturing while it runs and sends rows at most 60 times a second", async () => {
    const { backend, events } = await capturing(10_000);
    await backend.disconnect();
    const rows = pdRows(events);
    expect(rows.length).toBeGreaterThan(100);
    const lastSecond = rows.filter((r) => r.elapsed_ms >= 9_000);
    expect(lastSecond.length).toBeGreaterThan(0);
    expect(pdBatches(events)).toBeLessThanOrEqual(600);
    expect(pdBatches(events)).toBeLessThan(rows.length);
  });

  it("stops sending rows once stopped", async () => {
    const { backend, events } = await capturing(1_000);
    await backend.pdStop();
    const n = pdRows(events).length;
    vi.advanceTimersByTime(3_000);
    expect(pdRows(events).length).toBe(n);
    await backend.disconnect();
  });
});

describe("MockBackend PD details (AC-TGE83C.4)", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("answers raw bytes and a field tree with byte ranges", async () => {
    const { backend, events } = await capturing(2_000);
    const caps = pdRows(events).find((r) => r.name === "Source_Capabilities")!;
    const d = await backend.pdDetails(caps.index);
    expect(d.index).toBe(caps.index);
    expect(d.raw).toHaveLength(2 + 4 * (caps.objects ?? 0));
    expect(d.raw[0]! & 0x1f).toBe(1);
    const header = find(d.fields, "Header");
    expect(header.bytes).toEqual([0, 2]);
    expect(find(header.children, "Message Type").bytes).toEqual([0, 1]);
    const pdo1 = find(d.fields, "PDO 1");
    expect(pdo1.bytes).toEqual([2, 6]);
    expect(find(pdo1.children, "Voltage").bytes).toEqual([3, 5]);
    await backend.disconnect();
  });

  it("rejects an unknown index", async () => {
    const { backend } = await capturing(500);
    await expect(backend.pdDetails(1_000_000)).rejects.toMatchObject({ kind: "usage" });
    await backend.disconnect();
  });
});

describe("MockBackend PD clear and seed (AC-TGE83C.6, AC-TGE83C.7)", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("clear drops the captured records", async () => {
    const { backend } = await capturing(2_000);
    await backend.pdStop();
    await backend.pdClear();
    await expect(backend.pdDetails(0)).rejects.toMatchObject({ kind: "usage" });
    await backend.disconnect();
  });

  it("?pdsynthetic=<n> seeds n records at the capture start, then the live capture goes on", async () => {
    expect(pdSyntheticFromUrl("?pdsynthetic=100000")).toBe(100_000);
    expect(pdSyntheticFromUrl("?pdsynthetic=abc")).toBeNull();
    expect(pdSyntheticFromUrl("")).toBeNull();
    const backend = new MockBackend(null, { pdSynthetic: 100_000 });
    const events: BackendEvent[] = [];
    await backend.connect("demo:load-5v-1a", 50, (e) => events.push(e));
    await backend.pdStart();
    vi.advanceTimersByTime(1_000);
    const rows = pdRows(events);
    expect(rows.length).toBeGreaterThan(100_000);
    expect(rows.slice(0, 100_000).filter((r) => r.name === "GoodCRC").length).toBeGreaterThan(0);
    expect(rows.map((r) => r.index).slice(99_995, 100_005)).toEqual(
      Array.from({ length: 10 }, (_, i) => 99_995 + i),
    );
    // The seed's records answer details like live ones.
    const d = await backend.pdDetails(50_000);
    expect(d.raw.length).toBeGreaterThan(0);
    await backend.disconnect();
  });
});
