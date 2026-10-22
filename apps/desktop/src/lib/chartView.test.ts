import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { BackendEvent, SeriesBuckets } from "./backend";
import { LiveBuffer, LIVE_CAPACITY, liveRange } from "./chartData";
import {
  HistoryRequester,
  MAX_REDRAW_HZ,
  RateGate,
  RedrawScheduler,
  applyGesture,
  frameSource,
  historyRequestFor,
  rangeAnnouncement,
  rangeDecimals,
  sessionRequestFor,
  sessionStartOf,
  windowPressed,
  type ChartView,
  type FrameClock,
  type HistoryRequest,
} from "./chartView";
import { MockBackend } from "./mockBackend";
import { shortcutFor } from "./shortcuts";

/** A hand-driven requestAnimationFrame. */
class FakeFrames implements FrameClock {
  #next = 1;
  #pending = new Map<number, (ts: number) => void>();
  request(cb: (ts: number) => void): number {
    const id = this.#next++;
    this.#pending.set(id, cb);
    return id;
  }
  cancel(id: number): void {
    this.#pending.delete(id);
  }
  get pending(): number {
    return this.#pending.size;
  }
  tick(ts: number): void {
    const due = [...this.#pending.values()];
    this.#pending.clear();
    for (const cb of due) cb(ts);
  }
}

const LIVE: ChartView = { mode: "live", window: "10s" };
const SHOWN = { t0Ms: 10_000, t1Ms: 20_000 };
const SESSION = { firstMs: 0, lastMs: 30_000 };

function span(v: ChartView): number {
  if (v.mode !== "history") throw new Error("expected a history view");
  return v.t1Ms - v.t0Ms;
}

describe("redraw rate (AC-AH2AEM.2)", () => {
  it("caps redraws at 60 per second on a 144 Hz display even when every frame is dirty", () => {
    const frames = new FakeFrames();
    let draws = 0;
    const scheduler = new RedrawScheduler(() => draws++, frames);
    for (let i = 0; i < 144; i++) {
      scheduler.invalidate();
      frames.tick((i * 1000) / 144);
    }
    expect(MAX_REDRAW_HZ).toBe(60);
    expect(draws).toBeLessThanOrEqual(60);
    expect(draws).toBeGreaterThanOrEqual(20);
  });

  it("draws nothing while nothing changed, and coalesces many invalidations into one frame", () => {
    const frames = new FakeFrames();
    let draws = 0;
    const scheduler = new RedrawScheduler(() => draws++, frames);
    frames.tick(0);
    expect(draws).toBe(0);
    scheduler.invalidate();
    scheduler.invalidate();
    scheduler.invalidate();
    expect(frames.pending).toBe(1);
    frames.tick(16.7);
    expect(draws).toBe(1);
    frames.tick(33.4);
    expect(draws).toBe(1);
  });

  it("stops drawing once stopped", () => {
    const frames = new FakeFrames();
    let draws = 0;
    const scheduler = new RedrawScheduler(() => draws++, frames);
    scheduler.invalidate();
    scheduler.stop();
    frames.tick(0);
    scheduler.invalidate();
    frames.tick(20);
    expect(draws).toBe(0);
  });

  describe("with the demo device streaming at 1000 SPS", () => {
    beforeEach(() => vi.useFakeTimers());
    afterEach(() => vi.useRealTimers());

    it("scrolls the live window and redraws between 20 and 60 times per second", async () => {
      const backend = new MockBackend();
      const frames = new FakeFrames();
      const buf = new LiveBuffer(LIVE_CAPACITY);
      const drawnEnds: number[] = [];
      const scheduler = new RedrawScheduler(() => {
        const range = liveRange("10s", buf);
        if (range) drawnEnds.push(range[1]);
      }, frames);
      await backend.connect("demo:load-5v-1a", 1000, (e: BackendEvent) => {
        if (e.type === "batch") {
          buf.push(e.data.samples);
          scheduler.invalidate();
        }
      });
      // 2 s of a 60 Hz display; count the second second.
      const frameMs = 1000 / 60;
      let drawsBefore = 0;
      for (let i = 1; i <= 120; i++) {
        vi.advanceTimersByTime(frameMs);
        frames.tick(i * frameMs);
        if (i === 60) drawsBefore = drawnEnds.length;
      }
      await backend.disconnect();
      const perSecond = drawnEnds.length - drawsBefore;
      expect(perSecond).toBeGreaterThanOrEqual(20);
      expect(perSecond).toBeLessThanOrEqual(60);
      // It scrolls: the window's right edge keeps moving forward.
      expect(drawnEnds.at(-1)).toBeGreaterThan(drawnEnds[drawsBefore] ?? Infinity);
      expect(buf.lastMs).toBeGreaterThan(1_900);
    });
  });
});

describe("gestures leave live mode (AC-AH2AEM.3)", () => {
  it("drag-to-zoom shows exactly the selected range", () => {
    const v = applyGesture(LIVE, { kind: "select", t0Ms: 12_000, t1Ms: 15_000 }, SHOWN, SESSION);
    expect(v).toEqual({ mode: "history", window: "10s", t0Ms: 12_000, t1Ms: 15_000 });
  });

  it("wheel up zooms in around the pointer, wheel down zooms out", () => {
    const zin = applyGesture(LIVE, { kind: "wheel", atMs: 12_500, deltaY: -100 }, SHOWN, SESSION);
    expect(zin.mode).toBe("history");
    expect(span(zin)).toBeLessThan(10_000);
    if (zin.mode === "history") {
      // the pointer stays at the same fraction (25 %) of the view
      expect((12_500 - zin.t0Ms) / (zin.t1Ms - zin.t0Ms)).toBeCloseTo(0.25, 6);
    }
    const zout = applyGesture(LIVE, { kind: "wheel", atMs: 15_000, deltaY: 100 }, SHOWN, SESSION);
    expect(span(zout)).toBeGreaterThan(10_000);
  });

  it("drag-to-pan shifts the view without changing its span", () => {
    const v = applyGesture(LIVE, { kind: "pan", deltaMs: -3_000 }, SHOWN, SESSION);
    expect(v).toEqual({ mode: "history", window: "10s", t0Ms: 7_000, t1Ms: 17_000 });
  });

  it("arrow keys pan left/right and zoom in/out", () => {
    const left = applyGesture(LIVE, { kind: "key", key: "ArrowLeft" }, SHOWN, SESSION);
    const right = applyGesture(LIVE, { kind: "key", key: "ArrowRight" }, SHOWN, SESSION);
    const up = applyGesture(LIVE, { kind: "key", key: "ArrowUp" }, SHOWN, SESSION);
    const down = applyGesture(LIVE, { kind: "key", key: "ArrowDown" }, SHOWN, SESSION);
    for (const v of [left, right, up, down]) expect(v.mode).toBe("history");
    if (left.mode === "history") expect(left.t0Ms).toBeLessThan(SHOWN.t0Ms);
    if (right.mode === "history") expect(right.t0Ms).toBeGreaterThan(SHOWN.t0Ms);
    expect(span(left)).toBe(10_000);
    expect(span(up)).toBeLessThan(10_000);
    expect(span(down)).toBeGreaterThan(10_000);
  });

  it("ignores keys that are not chart keys", () => {
    expect(applyGesture(LIVE, { kind: "key", key: "x" }, SHOWN, SESSION)).toEqual(LIVE);
  });

  it("the Live gesture (button or End key) returns to the remembered live window", () => {
    const zoomed = applyGesture(
      { mode: "live", window: "1m" },
      { kind: "select", t0Ms: 1_000, t1Ms: 2_000 },
      SHOWN,
      SESSION,
    );
    expect(applyGesture(zoomed, { kind: "live" }, SHOWN, SESSION)).toEqual({
      mode: "live",
      window: "1m",
    });
    // L is the shuttle since FR FENCD9; End goes live.
    expect(
      shortcutFor({ key: "End", ctrlKey: false, metaKey: false, altKey: false, target: null }),
    ).toEqual({ kind: "live" });
  });

  it("asks history_range for as many buckets as the plot is wide in pixels", () => {
    const view: ChartView = { mode: "history", window: "10s", t0Ms: 1_000, t1Ms: 9_000 };
    const req = historyRequestFor(view, 873);
    expect(req).toMatchObject({ t0Ms: 1_000, t1Ms: 9_000, buckets: 873 });
    expect(req?.series).toEqual(expect.arrayContaining(["vbus", "ibus", "dp", "dm", "cc1", "cc2"]));
    expect(historyRequestFor(view, 873.6)?.buckets).toBe(873);
    expect(historyRequestFor(view, 0)).toBeNull();
  });
});

describe("HistoryRequester (AC-AH2AEM.4)", () => {
  interface Call {
    req: HistoryRequest;
    resolve: (r: SeriesBuckets[]) => void;
    reject: (e: unknown) => void;
  }

  function setup() {
    const calls: Call[] = [];
    const results: { req: HistoryRequest; result: SeriesBuckets[] }[] = [];
    const errors: unknown[] = [];
    let inFlight = 0;
    let maxInFlight = 0;
    const requester = new HistoryRequester({
      fetch: (req) =>
        new Promise<SeriesBuckets[]>((resolve, reject) => {
          inFlight++;
          maxInFlight = Math.max(maxInFlight, inFlight);
          calls.push({
            req,
            resolve: (r) => {
              inFlight--;
              resolve(r);
            },
            reject: (e) => {
              inFlight--;
              reject(e);
            },
          });
        }),
      onResult: (req, result) => results.push({ req, result }),
      onError: (e) => errors.push(e),
      schedule: (cb) => cb(),
    });
    return { calls, results, errors, requester, maxInFlight: () => maxInFlight };
  }

  const req = (t0Ms: number): HistoryRequest => ({
    series: ["vbus", "ibus"],
    t0Ms,
    t1Ms: t0Ms + 1_000,
    buckets: 800,
  });
  const answer = (t: number): SeriesBuckets[] => [
    { series: "vbus", buckets: [{ t_ms: t, min: 5, max: 5 }] },
    { series: "ibus", buckets: [{ t_ms: t, min: 1, max: 1 }] },
  ];
  const settle = () => new Promise<void>((r) => setTimeout(r, 0));

  it("sends a request right away when none is in flight and delivers its answer", async () => {
    const { calls, results, requester } = setup();
    requester.request(req(0));
    expect(calls).toHaveLength(1);
    calls[0]?.resolve(answer(0));
    await settle();
    expect(results).toEqual([{ req: req(0), result: answer(0) }]);
  });

  it("keeps at most one request in flight; the newest pending one supersedes older ones", async () => {
    const { calls, results, requester, maxInFlight } = setup();
    requester.request(req(0));
    requester.request(req(1_000));
    requester.request(req(2_000));
    expect(calls).toHaveLength(1);
    calls[0]?.resolve(answer(0));
    await settle();
    // the stale answer for req(0) is dropped, req(1_000) is never sent
    expect(results).toEqual([]);
    expect(calls).toHaveLength(2);
    expect(calls[1]?.req).toEqual(req(2_000));
    calls[1]?.resolve(answer(2_000));
    await settle();
    expect(results).toEqual([{ req: req(2_000), result: answer(2_000) }]);
    expect(maxInFlight()).toBe(1);
  });

  it("drops an answer that arrives after cancel (e.g. back to live)", async () => {
    const { calls, results, requester } = setup();
    requester.request(req(0));
    requester.request(req(5_000));
    requester.cancel();
    calls[0]?.resolve(answer(0));
    await settle();
    expect(results).toEqual([]);
    expect(calls).toHaveLength(1);
  });

  it("reports a failed request and keeps serving newer ones", async () => {
    const { calls, results, errors, requester } = setup();
    requester.request(req(0));
    calls[0]?.reject({ kind: "usage", message: "No meter is connected.", hint: null });
    await settle();
    expect(errors).toHaveLength(1);
    requester.request(req(3_000));
    expect(calls).toHaveLength(2);
    calls[1]?.resolve(answer(3_000));
    await settle();
    expect(results.map((r) => r.req)).toEqual([req(3_000)]);
  });

  it("schedules the send on the next frame", () => {
    const frames: (() => void)[] = [];
    const calls: HistoryRequest[] = [];
    const requester = new HistoryRequester({
      fetch: (r) => {
        calls.push(r);
        return new Promise<SeriesBuckets[]>(() => {});
      },
      onResult: () => {},
      schedule: (cb) => frames.push(cb),
    });
    requester.request(req(0));
    requester.request(req(1_000));
    expect(calls).toHaveLength(0);
    for (const f of frames.splice(0)) f();
    expect(calls).toEqual([req(1_000)]);
  });
});

describe("views stay inside the session (AC-AH2AEM.3)", () => {
  const session = { firstMs: -50_000, lastMs: 20_000 };

  it("zooms out as far as the whole session history, not just the live buffer", () => {
    let v: ChartView = LIVE;
    let shown = SHOWN;
    for (let i = 0; i < 40; i++) {
      v = applyGesture(v, { kind: "key", key: "ArrowDown" }, shown, session);
      if (v.mode === "history") shown = { t0Ms: v.t0Ms, t1Ms: v.t1Ms };
    }
    expect(shown).toEqual({ t0Ms: -50_000, t1Ms: 20_000 });
  });

  it("cannot pan past either end of the session", () => {
    const left = applyGesture(LIVE, { kind: "pan", deltaMs: -1_000_000 }, SHOWN, session);
    expect(left).toMatchObject({ t0Ms: -50_000, t1Ms: -40_000 });
    const right = applyGesture(LIVE, { kind: "key", key: "ArrowRight" }, SHOWN, session);
    expect(right).toMatchObject({ t0Ms: 10_000, t1Ms: 20_000 });
  });

  it("clips a drag selection to the session", () => {
    const v = applyGesture(LIVE, { kind: "select", t0Ms: 15_000, t1Ms: 25_000 }, SHOWN, session);
    expect(v).toMatchObject({ t0Ms: 15_000, t1Ms: 20_000 });
  });
});

describe("window buttons", () => {
  it("presses the chosen window only in live mode", () => {
    expect(windowPressed({ mode: "live", window: "1m" }, "1m")).toBe(true);
    expect(windowPressed({ mode: "live", window: "1m" }, "10s")).toBe(false);
    const zoomed: ChartView = { mode: "history", window: "1m", t0Ms: 0, t1Ms: 1 };
    expect(windowPressed(zoomed, "1m")).toBe(false);
  });
});

describe("whole session beyond the live buffer (AC-AH2AEM.2)", () => {
  it("asks history_range for the whole session, one bucket per pixel", () => {
    const req = sessionRequestFor(90_000, 640.7);
    expect(req).toMatchObject({ t0Ms: 0, t1Ms: 90_000, buckets: 640, purpose: "session" });
    expect(sessionRequestFor(90_000, 0)?.buckets).toBe(1);
    expect(
      historyRequestFor({ mode: "history", window: "all", t0Ms: 0, t1Ms: 1 }, 10)?.purpose,
    ).toBe("view");
  });

  it("learns the session start from the first bucket", () => {
    expect(
      sessionStartOf([
        { series: "vbus", buckets: [{ t_ms: 120, min: 1, max: 1 }] },
        { series: "ibus", buckets: [{ t_ms: 100, min: 1, max: 1 }] },
      ]),
    ).toBe(100);
    expect(sessionStartOf([{ series: "vbus", buckets: [] }])).toBeNull();
  });

  it("picks the frame to draw", () => {
    const live: ChartView = { mode: "live", window: "all" };
    const base = {
      sessionStartMs: 0,
      bufferFirstMs: 5_000,
      hasHistoryFrame: false,
      hasSessionFrame: true,
    };
    expect(frameSource(live, base)).toBe("session");
    expect(frameSource(live, { ...base, sessionStartMs: 5_000 })).toBe("live");
    expect(frameSource(live, { ...base, sessionStartMs: null })).toBe("live");
    expect(frameSource(live, { ...base, hasSessionFrame: false })).toBe("live");
    expect(frameSource({ mode: "live", window: "10s" }, base)).toBe("live");
    const zoomed: ChartView = { mode: "history", window: "all", t0Ms: 0, t1Ms: 1 };
    // no answer yet (e.g. the plot had no width): the live samples stand in
    expect(frameSource(zoomed, base)).toBe("live");
    expect(frameSource(zoomed, { ...base, hasHistoryFrame: true })).toBe("history");
  });

  it("refreshes at a modest rate", () => {
    const gate = new RateGate(250);
    expect(gate.ready(1_000)).toBe(true);
    expect(gate.ready(1_100)).toBe(false);
    expect(gate.ready(1_250)).toBe(true);
    gate.reset();
    expect(gate.ready(1_260)).toBe(true);
  });
});

describe("range announcement", () => {
  it("says the visible range in history mode and the window in live mode", () => {
    const zoomed: ChartView = { mode: "history", window: "10s", t0Ms: 1_500, t1Ms: 4_250 };
    expect(rangeAnnouncement(zoomed)).toBe("History: 1.5 s to 4.3 s");
    expect(rangeAnnouncement({ mode: "live", window: "1m" })).toBe("Live: 1 min");
  });
});

describe("range announcement counts from the session start (AC-272WSN.2)", () => {
  it("reads the history range relative to the origin, never the meter's clock", () => {
    const zoomed: ChartView = {
      mode: "history",
      window: "10s",
      t0Ms: 30_401_500,
      t1Ms: 30_404_250,
    };
    expect(rangeAnnouncement(zoomed, 30_400_000)).toBe("History: 1.5 s to 4.3 s");
  });

  it("switches to m:ss and h:mm:ss like the axis once the range passes a minute or an hour", () => {
    const origin = 30_400_000;
    const later: ChartView = {
      mode: "history",
      window: "10s",
      t0Ms: origin + 62_000,
      t1Ms: origin + 3_723_400,
    };
    expect(rangeAnnouncement(later, origin)).toBe("History: 1:02.0 to 1:02:03.4");
  });
});

describe("HistoryRequester after cancel", () => {
  it("schedules nothing more once the cancelled request settles", async () => {
    let resolve: (r: SeriesBuckets[]) => void = () => {};
    let scheduled = 0;
    const requester = new HistoryRequester({
      fetch: () => new Promise<SeriesBuckets[]>((r) => (resolve = r)),
      onResult: () => {},
      schedule: (cb) => {
        scheduled++;
        cb();
      },
    });
    requester.request({ series: ["vbus"], t0Ms: 0, t1Ms: 1, buckets: 1 });
    expect(scheduled).toBe(1);
    requester.cancel();
    resolve([]);
    await new Promise<void>((r) => setTimeout(r, 0));
    expect(scheduled).toBe(1);
  });
});

describe("live and paused chart states (FR DDDYDJ)", () => {
  const live: ChartView = { mode: "live", window: "1m" };
  const paused: ChartView = { mode: "history", window: "1m", t0Ms: 10_000, t1Ms: 20_000 };

  it("AC-DDDYDJ.1: while live the button reads Live and is pressed", async () => {
    const { liveButton } = await import("./chartView");
    expect(liveButton(live)).toMatchObject({ label: "Live", pressed: true });
    expect(liveButton({ mode: "live", window: "10s" })).toMatchObject({
      label: "Live",
      pressed: true,
    });
  });

  it("AC-DDDYDJ.1: a live view draws the live frame (follows the newest data)", () => {
    expect(
      frameSource(live, {
        sessionStartMs: 0,
        bufferFirstMs: 0,
        hasHistoryFrame: true,
        hasSessionFrame: true,
      }),
    ).toBe("live");
  });

  it("AC-DDDYDJ.2: the button while live pauses on exactly the range on screen", async () => {
    const { liveToggle } = await import("./chartView");
    const gesture = liveToggle(live);
    expect(gesture).toEqual({ kind: "pause" });
    expect(applyGesture(live, gesture, SHOWN, SESSION)).toEqual({
      mode: "history",
      window: "1m",
      t0Ms: SHOWN.t0Ms,
      t1Ms: SHOWN.t1Ms,
    });
  });

  it("AC-DDDYDJ.2: pausing a view that is already paused changes nothing", () => {
    expect(applyGesture(paused, { kind: "pause" }, { t0Ms: 1, t1Ms: 2 }, SESSION)).toBe(paused);
  });

  it("AC-DDDYDJ.2: the live tooltip says the stream and any recording keep running", async () => {
    const { liveButton } = await import("./chartView");
    expect(liveButton(live).title).toMatch(/pause/i);
    expect(liveButton(live).title).toMatch(/stream and any recording keep running/i);
  });

  it("AC-DDDYDJ.3: wheel, drag and arrow keys all pause the chart", async () => {
    const { liveButton } = await import("./chartView");
    const gestures = [
      { kind: "wheel", atMs: 15_000, deltaY: -100 },
      { kind: "pan", deltaMs: -2_000 },
      { kind: "select", t0Ms: 12_000, t1Ms: 16_000 },
      { kind: "key", key: "ArrowLeft" },
      { kind: "key", key: "ArrowRight" },
      { kind: "key", key: "ArrowUp" },
      { kind: "key", key: "ArrowDown" },
    ] as const;
    for (const g of gestures) {
      const v = applyGesture(live, g, SHOWN, SESSION);
      expect(v.mode).toBe("history");
      expect(liveButton(v)).toMatchObject({ label: "Paused · Go live", pressed: false });
    }
  });

  it("AC-DDDYDJ.4: while paused the button reads Paused · Go live, unpressed, with a how-to tooltip", async () => {
    const { liveButton } = await import("./chartView");
    const b = liveButton(paused);
    expect(b).toMatchObject({ label: "Paused · Go live", pressed: false });
    expect(b.title).toMatch(/live/i);
    expect(b.title).toMatch(/End/);
  });

  it("AC-DDDYDJ.5: the button while paused returns to live, keeping the window", async () => {
    const { liveToggle } = await import("./chartView");
    const gesture = liveToggle(paused);
    expect(gesture).toEqual({ kind: "live" });
    expect(applyGesture(paused, gesture, SHOWN, SESSION)).toEqual(live);
  });

  it("AC-DDDYDJ.5: End on the focused chart is a chart key and returns to live", async () => {
    const { CHART_KEYS } = await import("./chartView");
    expect(CHART_KEYS).toContain("End");
    expect(applyGesture(paused, { kind: "key", key: "End" }, SHOWN, SESSION)).toEqual(live);
  });

  it("AC-DDDYDJ.6: the status says Paused: <from> to <to> when the button pauses", () => {
    expect(rangeAnnouncement(paused, 0, { paused: true })).toBe("Paused: 10.0 s to 20.0 s");
    expect(rangeAnnouncement(paused, 4_000, { paused: true })).toBe("Paused: 6.0 s to 16.0 s");
  });

  it("AC-DDDYDJ.6: a zoom or pan still announces History, going live announces Live: <window>", () => {
    expect(rangeAnnouncement(paused, 0)).toBe("History: 10.0 s to 20.0 s");
    expect(rangeAnnouncement(live, 0, { paused: false })).toBe("Live: 1 min");
  });
});

describe("going live while already live changes nothing (AC-DDDYDJ.5)", () => {
  it("returns the same view, so a repeated Live, L or End costs no redraw", () => {
    const live: ChartView = { mode: "live", window: "1m" };
    expect(applyGesture(live, { kind: "live" }, SHOWN, SESSION)).toBe(live);
    expect(applyGesture(live, { kind: "key", key: "End" }, SHOWN, SESSION)).toBe(live);
  });
});

describe("timeline gestures (FR FENCD9)", () => {
  const live: ChartView = { mode: "live", window: "10s" };
  /** Gestures the TS union may not know yet, applied as the contract defines them. */
  const apply = (v: ChartView, g: Record<string, unknown>): ChartView =>
    applyGesture(v, g as unknown as Parameters<typeof applyGesture>[1], SHOWN, SESSION);

  it("AC-FENCD9.2: a range gesture shows exactly that range, paused", () => {
    expect(apply(live, { kind: "range", t0Ms: 5_000, t1Ms: 8_000 })).toEqual({
      mode: "history",
      window: "10s",
      t0Ms: 5_000,
      t1Ms: 8_000,
    });
  });

  it("AC-FENCD9.2: a range gesture stays inside the session", () => {
    const v = apply(live, { kind: "range", t0Ms: 25_000, t1Ms: 35_000 });
    expect(v.mode).toBe("history");
    if (v.mode !== "history") return;
    expect(v.t0Ms).toBeGreaterThanOrEqual(SESSION.firstMs);
    expect(v.t1Ms).toBeLessThanOrEqual(SESSION.lastMs);
    expect(v.t1Ms).toBe(SESSION.lastMs);
  });

  it("AC-FENCD9.8: a zoom gesture keeps atMs in place; factor < 1 zooms in", () => {
    const zin = apply(live, { kind: "zoom", factor: 0.5, atMs: 12_500 });
    expect(span(zin)).toBeCloseTo(5_000, 6);
    if (zin.mode === "history") {
      expect((12_500 - zin.t0Ms) / (zin.t1Ms - zin.t0Ms)).toBeCloseTo(0.25, 6);
    }
    const zout = apply(live, { kind: "zoom", factor: 1 / 0.8, atMs: 15_000 });
    expect(span(zout)).toBeCloseTo(12_500, 6);
  });

  it("AC-FENCD9.8: fit shows the whole session, live", () => {
    const paused: ChartView = { mode: "history", window: "1m", t0Ms: 10_000, t1Ms: 12_000 };
    expect(apply(paused, { kind: "fit" })).toEqual({ mode: "live", window: "all" });
    expect(apply(live, { kind: "fit" })).toEqual({ mode: "live", window: "all" });
  });

  it("AC-FENCD9.8: home jumps to the session start, same span, paused", () => {
    expect(apply(live, { kind: "home" })).toEqual({
      mode: "history",
      window: "10s",
      t0Ms: 0,
      t1Ms: 10_000,
    });
  });

  it("AC-FENCD9.3: a horizontal scroll pans by the scroll, keeping the span", () => {
    const right = apply(live, { kind: "wheel", atMs: 15_000, deltaY: 0, deltaX: 100 });
    expect(right.mode).toBe("history");
    expect(span(right)).toBeCloseTo(10_000, 6);
    if (right.mode === "history") expect(right.t0Ms).toBeGreaterThan(SHOWN.t0Ms);
    const left = apply(live, { kind: "wheel", atMs: 15_000, deltaY: 0, deltaX: -100 });
    expect(span(left)).toBeCloseTo(10_000, 6);
    if (left.mode === "history") expect(left.t0Ms).toBeLessThan(SHOWN.t0Ms);
  });

  it("AC-FENCD9.3: Shift+wheel pans instead of zooming", () => {
    const down = apply(live, { kind: "wheel", atMs: 15_000, deltaY: 100, shiftKey: true });
    expect(down.mode).toBe("history");
    expect(span(down)).toBeCloseTo(10_000, 6);
    if (down.mode === "history") expect(down.t0Ms).toBeGreaterThan(SHOWN.t0Ms);
    const up = apply(live, { kind: "wheel", atMs: 15_000, deltaY: -100, shiftKey: true });
    expect(span(up)).toBeCloseTo(10_000, 6);
    if (up.mode === "history") expect(up.t0Ms).toBeLessThan(SHOWN.t0Ms);
  });

  it("AC-FENCD9.3: a plain vertical wheel still zooms", () => {
    const v = apply(live, {
      kind: "wheel",
      atMs: 15_000,
      deltaY: -100,
      deltaX: 0,
      shiftKey: false,
    });
    expect(span(v)).toBeLessThan(10_000);
  });

  it("AC-FENCD9.2/.3/.8: every range change pauses the chart", async () => {
    const { liveButton } = await import("./chartView");
    const gestures = [
      { kind: "range", t0Ms: 1_000, t1Ms: 4_000 },
      { kind: "zoom", factor: 0.8, atMs: 15_000 },
      { kind: "zoom", factor: 1 / 0.8, atMs: 15_000 },
      { kind: "home" },
      { kind: "wheel", atMs: 15_000, deltaY: 0, deltaX: 50 },
      { kind: "wheel", atMs: 15_000, deltaY: 50, shiftKey: true },
    ];
    for (const g of gestures) {
      const v = apply(live, g);
      expect(v.mode, JSON.stringify(g)).toBe("history");
      expect(liveButton(v)).toMatchObject({ label: "Paused · Go live", pressed: false });
    }
  });
});

describe("the narrowest view is the zoom slider's bottom (AC-FENCD9.9)", () => {
  it("never zooms narrower than 10 ms, by wheel or key", async () => {
    const { MIN_SPAN_MS } = await import("./timeline");
    const narrow: ChartView = { mode: "history", window: "10s", t0Ms: 10_000, t1Ms: 10_012 };
    const shown = { t0Ms: 10_000, t1Ms: 10_012 };
    const wheel = applyGesture(
      narrow,
      { kind: "wheel", atMs: 10_006, deltaY: -100 },
      shown,
      SESSION,
    );
    const zoom = applyGesture(narrow, { kind: "zoom", factor: 0.01, atMs: 10_006 }, shown, SESSION);
    for (const v of [wheel, zoom]) {
      if (v.mode !== "history") throw new Error("expected a paused view");
      expect(v.t1Ms - v.t0Ms).toBeGreaterThanOrEqual(MIN_SPAN_MS - 1e-9);
    }
  });
});

describe("history views ask for Power (AC-JFD9XR.4)", () => {
  it("requests the power series with the stored ones", () => {
    const view: ChartView = { mode: "history", window: "10s", t0Ms: 1_000, t1Ms: 5_000 };
    const req = historyRequestFor(view, 600);
    expect(req?.series).toContain("power");
    for (const s of ["vbus", "ibus", "dp", "dm", "cc1", "cc2"]) expect(req?.series).toContain(s);
  });
});

describe("range announcement per view kind (AC-JFD9XR.6)", () => {
  it("names each live window", () => {
    expect(rangeAnnouncement({ mode: "live", window: "10s" })).toBe("Live: 10 s");
    expect(rangeAnnouncement({ mode: "live", window: "1m" })).toBe("Live: 1 min");
    expect(rangeAnnouncement({ mode: "live", window: "10m" })).toBe("Live: 10 min");
    expect(rangeAnnouncement({ mode: "live", window: "all" })).toBe("Live: Whole session");
  });

  it("names a paused range from the session start", () => {
    const paused: ChartView = { mode: "history", window: "1m", t0Ms: 12_500, t1Ms: 72_500 };
    expect(rangeAnnouncement(paused, 2_500, { paused: true })).toBe("Paused: 10.0 s to 1:10.0");
  });
});

describe("applyGesture never zooms past two sample periods (AC-9CFMD3.4)", () => {
  const AT_50SPS = { ...SESSION, minSpanMs: 40 };

  it("clamps wheel and key zooms to the rate's narrowest view", () => {
    let view: ChartView = LIVE;
    let shown = SHOWN;
    for (let k = 0; k < 100; k++) {
      view = applyGesture(view, { kind: "wheel", atMs: 15_000, deltaY: -100 }, shown, AT_50SPS);
      if (view.mode === "history") shown = { t0Ms: view.t0Ms, t1Ms: view.t1Ms };
    }
    expect(span(view)).toBeCloseTo(40, 6);
    for (let k = 0; k < 100; k++) {
      view = applyGesture(view, { kind: "key", key: "ArrowUp" }, shown, AT_50SPS);
      if (view.mode === "history") shown = { t0Ms: view.t0Ms, t1Ms: view.t1Ms };
    }
    expect(span(view)).toBeCloseTo(40, 6);
  });

  it("refuses a selection or range narrower than two sample periods", () => {
    const sel = applyGesture(LIVE, { kind: "select", t0Ms: 12_000, t1Ms: 12_020 }, SHOWN, AT_50SPS);
    expect(sel).toBe(LIVE);
    const range = applyGesture(
      LIVE,
      { kind: "range", t0Ms: 12_000, t1Ms: 12_030 },
      SHOWN,
      AT_50SPS,
    );
    expect(range).toBe(LIVE);
    const ok = applyGesture(LIVE, { kind: "select", t0Ms: 12_000, t1Ms: 12_040 }, SHOWN, AT_50SPS);
    expect(span(ok)).toBeCloseTo(40, 6);
  });
});

describe("range status decimals follow the visible span (AC-SX3EDV.5)", () => {
  it("uses one decimal from 1 s up, two from 100 ms to 1 s, three below 100 ms", () => {
    expect(rangeDecimals(3_600_000)).toBe(1);
    expect(rangeDecimals(10_000)).toBe(1);
    expect(rangeDecimals(1_000)).toBe(1);
    expect(rangeDecimals(999)).toBe(2);
    expect(rangeDecimals(250)).toBe(2);
    expect(rangeDecimals(100)).toBe(2);
    expect(rangeDecimals(99)).toBe(3);
    expect(rangeDecimals(40)).toBe(3);
    expect(rangeDecimals(1)).toBe(3);
  });

  it('reads "Paused: 11.200 s to 11.240 s" for a 40 ms pause, two different times', () => {
    const narrow: ChartView = { mode: "history", window: "10s", t0Ms: 11_200, t1Ms: 11_240 };
    expect(rangeAnnouncement(narrow, 0, { paused: true })).toBe("Paused: 11.200 s to 11.240 s");
  });

  it("writes two decimals for a span between 100 ms and 1 s", () => {
    const view: ChartView = { mode: "history", window: "10s", t0Ms: 11_200, t1Ms: 11_450 };
    expect(rangeAnnouncement(view)).toBe("History: 11.20 s to 11.45 s");
  });

  it("keeps one decimal from 1 s up, counting from the session start", () => {
    const view: ChartView = { mode: "history", window: "10s", t0Ms: 30_401_500, t1Ms: 30_402_500 };
    expect(rangeAnnouncement(view, 30_400_000)).toBe("History: 1.5 s to 2.5 s");
  });
});
