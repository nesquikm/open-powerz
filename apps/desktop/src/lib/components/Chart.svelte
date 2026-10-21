<script lang="ts">
  import uPlot from "uplot";
  import "uplot/dist/uPlot.min.css";
  import { getContext, untrack } from "svelte";
  import { historyGaps, historyRange, type Gap, type Sample } from "../backend";
  import { backfillRange, fetchBackfill } from "../backfill";
  import {
    GapList,
    LIVE_CAPACITY,
    LIVE_WINDOWS,
    LiveBuffer,
    bucketsToFrame,
    formatHoverValue,
    gapBands,
    gapLabelFits,
    hoverReadout,
    liveRange,
    steppedRange,
    toAlignedData,
    type ChartFrame,
    type GapBand,
    type HoverReadout,
    type LiveWindowId,
  } from "../chartData";
  import {
    CHART_SERIES,
    type ChartSeriesKey,
    type SeriesVisibility,
    chartOptions,
    formatSessionTime,
    formatSpan,
    loadLiveWindow,
    loadVisibility,
    loadZeroBased,
    saveLiveWindow,
    saveVisibility,
    saveZeroBased,
    seriesDef,
    seriesStyles,
    sessionStore,
    timeSplits,
    toggleSeries,
  } from "../chartSeries";
  import {
    CHART_KEYS,
    HistoryRequester,
    RateGate,
    RedrawScheduler,
    animationFrames,
    applyGesture,
    frameSource,
    historyRequestFor,
    liveButton,
    liveToggle,
    rangeAnnouncement,
    sessionRequestFor,
    sessionStartOf,
    windowPressed,
    type ChartGesture,
    type ChartView,
    type SessionExtent,
    type TimeRange,
  } from "../chartView";
  import {
    buildWarp,
    followPlayhead,
    minSpanMs,
    presetRange,
    shuttlePress,
    shuttleStep,
    sliderToSpan,
    spanToSlider,
    stepPlayhead,
    zoomInDisplay,
    type TimeWarp,
  } from "../timeline";
  import { E2E_BUILD, ChartProbe, installChartProbe } from "../e2eHook";
  import { THEME_COLORS, type MotionSettings, type ResolvedTheme } from "../theme";
  import OverviewStrip, { type EnvelopeSource, type OverviewState } from "./OverviewStrip.svelte";

  let {
    theme,
    rateSps = null,
    active = true,
  }: {
    theme: ResolvedTheme;
    rateSps?: number | null;
    /** A meter session is open; without one the chart shows how to start (AC-9CFMD3.5). */
    active?: boolean;
  } = $props();

  const motion = getContext<() => MotionSettings>("motion");

  /** The live samples; not reactive, the scheduler redraws when it changes. */
  const buffer = new LiveBuffer(LIVE_CAPACITY);
  /** This session's gaps, so history frames break where the stream did (AC-AH2AEM.7). */
  const sessionGaps = new GapList();
  /** The chosen live window, kept for the session (AC-AKNVND.5). */
  const storedWindow = loadLiveWindow(sessionStore());
  let liveWindow: LiveWindowId = $state(storedWindow);
  /** Live (scrolling) or a fixed history range picked by zoom / pan (AC-AH2AEM.3). */
  let view: ChartView = $state({ mode: "live", window: storedWindow });
  /** The last `history_range` answer drawn in history mode. */
  let historyFrame: ChartFrame | null = null;
  /** The range `historyFrame` answers. */
  let historyFrameRange: [number, number] | null = null;
  /** The last whole-session answer, drawn by "Whole session" once the session is
   * older than the live buffer (AC-AH2AEM.2). */
  let sessionFrame: ChartFrame | null = null;
  /** The session's first sample time, learned from the history (it may predate the
   * live buffer); `null` until the first whole-session answer. */
  let sessionStartMs: number | null = null;
  /** Whole-session refreshes while streaming: at most 4 per second. */
  const sessionGate = new RateGate(250);
  /** Why the last history request failed, shown over the plot (the last frame stays). */
  let historyError: string | null = $state(null);
  /** Screen-reader status: the visible range on entering history or returning to live. */
  let announcement = $state("");
  let host: HTMLDivElement | undefined = $state();
  let plot: uPlot | null = null;
  /** The frame on screen, for the hover readout. */
  let shownFrame: ChartFrame | null = null;
  /** Where session time starts, in x units: the time axis, the hover readout and the
   * announcements count from it, never from the meter's own clock (AC-272WSN.1, AC-272WSN.2). */
  function originMs(): number {
    return sessionStartMs ?? buffer.firstMs ?? 0;
  }
  /** The session's extent for a view on `[t0Ms, t1Ms]`: from its first known sample to
   * the newest (or the view's end, if later). */
  function sessionExtent(t0Ms: number, t1Ms: number): SessionExtent {
    return {
      firstMs: sessionStartMs ?? buffer.firstMs ?? t0Ms,
      lastMs: Math.max(buffer.lastMs ?? t1Ms, t1Ms),
      // The narrowest view: two sample periods at the current rate (AC-9CFMD3.4).
      minSpanMs: minSpanMs(rateSps),
    };
  }
  /** A window button's span in ms; `null` for the whole session. */
  function windowMs(id: LiveWindowId): number | null {
    return LIVE_WINDOWS.find((w) => w.id === id)?.ms ?? null;
  }
  /** Where a keyboard zoom, a step or a preset anchors: the playhead, else the centre of
   * `range`. */
  function anchorMs(range: [number, number]): number {
    return playheadMs ?? (range[0] + range[1]) / 2;
  }
  /** View announcements: at most 4 per second, the last one always made (AC-JFD9XR.6). */
  const ANNOUNCE_MS = 750;
  const announceGate = new RateGate(ANNOUNCE_MS);
  let announceTimer: ReturnType<typeof setTimeout> | null = null;
  /** Announces the view now on screen: "Live: 1 min", or "Paused: a to b" for any
   * non-live view; throttled, with a trailing update so the final view is announced. */
  function announceView(): void {
    if (announceTimer !== null) return;
    if (announceGate.ready(performance.now())) {
      announcement = rangeAnnouncement(view, originMs(), { paused: view.mode === "history" });
      return;
    }
    announceTimer = setTimeout(() => {
      announceTimer = null;
      announceView();
    }, ANNOUNCE_MS);
  }
  /** Drops a pending view announcement (a clear announces itself). */
  function cancelAnnouncement(): void {
    if (announceTimer !== null) clearTimeout(announceTimer);
    announceTimer = null;
  }
  $effect(() => () => cancelAnnouncement());
  /** Shows exactly `r`, paused. */
  function showRange(r: TimeRange): void {
    gesture({ kind: "range", t0Ms: r.t0Ms, t1Ms: r.t1Ms });
  }

  /** Zoom slider steps (AC-FENCD9.9). */
  const ZOOM_SLIDER_MAX = 1000;
  /** The zoom slider's position: the span on screen, on a log scale up to the session. */
  let zoomValue = $state(ZOOM_SLIDER_MAX);
  /** The span on screen as the slider's value text: "10.0 s", "1:00". */
  let zoomText = $state("");
  /** The plot took focus from a mouse press: no focus ring until focus leaves it. */
  let pointerFocus = $state(false);

  /** Moves the slider to the span on screen within the session; live, the window's
   * span (the buffer may not fill it yet). */
  function updateZoomSlider(range: [number, number] | null): void {
    if (range === null) return;
    const { firstMs, lastMs, minSpanMs: minSpan } = sessionExtent(range[0], range[1]);
    const sessionSpan = lastMs - firstMs;
    const span =
      view.mode === "live"
        ? Math.min(windowMs(liveWindow) ?? sessionSpan, sessionSpan)
        : range[1] - range[0];
    // The slider scales the chart as drawn: long gaps collapsed (AC-FENCD9.14).
    const { warp: whole, span: wholeShown } = wholeDisplay(firstMs, lastMs);
    // Live: the live window's own stretch of the session, measured as drawn too, so a
    // collapsed gap inside it counts as drawn on both sides of the comparison.
    const liveSpan = windowMs(liveWindow);
    const shownSpan =
      view.mode === "live"
        ? liveSpan === null
          ? wholeShown
          : Math.min(
              whole.fwd(lastMs) - whole.fwd(Math.max(firstMs, lastMs - liveSpan)),
              wholeShown,
            )
        : whole.fwd(range[1]) - whole.fwd(range[0]);
    const value = Math.round(spanToSlider(shownSpan, wholeShown, minSpan) * ZOOM_SLIDER_MAX);
    if (value !== zoomValue) zoomValue = value;
    const text = formatSpan(span);
    if (text !== zoomText) zoomText = text;
  }

  /** The slider moved: that span around the view centre, paused. */
  function onZoomInput(e: Event): void {
    const range = currentRange();
    if (range === null) return;
    const value = Number((e.currentTarget as HTMLInputElement).value) / ZOOM_SLIDER_MAX;
    const session = sessionExtent(range[0], range[1]);
    // Positions map to spans of the chart as drawn, the top to the whole session as
    // drawn, zoomed around the view's centre on screen (AC-FENCD9.14).
    const { warp: whole, span: wholeShown } = wholeDisplay(session.firstMs, session.lastMs);
    const d0 = whole.fwd(range[0]);
    const d1 = whole.fwd(range[1]);
    const target = sliderToSpan(value, wholeShown, session.minSpanMs);
    // A view with no width on the chart has nothing to scale.
    if (!(d1 > d0)) return;
    const r = zoomInDisplay(
      { t0Ms: range[0], t1Ms: range[1] },
      whole.bwd((d0 + d1) / 2),
      target / (d1 - d0),
      whole,
      session,
    );
    gesture({ kind: "range", t0Ms: r.t0Ms, t1Ms: r.t1Ms });
  }

  /** The whole session drawn across the plot, the zoom slider's scale: its warp and
   * its span as drawn (long gaps collapsed). */
  function wholeDisplay(firstMs: number, lastMs: number): { warp: TimeWarp; span: number } {
    // Every draw asks (the slider follows the view): rebuild only when the session, the
    // plot width or the gap list changed.
    const width = plotWidthPx();
    const key = `${firstMs}|${lastMs}|${width}|${sessionGaps.version}`;
    if (wholeCache === null || wholeCache.key !== key) {
      const warp = warpFor(firstMs, lastMs, width);
      wholeCache = { key, warp, span: warp.fwd(lastMs) - warp.fwd(firstMs) };
    }
    return wholeCache;
  }
  /** The whole-session map the zoom slider last used, and what it was built for. */
  let wholeCache: { key: string; warp: TimeWarp; span: number } | null = null;

  /** The playhead, session ms: placed by a click on the plot, moved by ← / →
   * (AC-FENCD9.6); `null` until placed. */
  let playheadMs: number | null = null;

  /** Places the playhead and redraws. */
  function setPlayhead(ms: number | null): void {
    playheadMs = ms;
    if (E2E_BUILD && probe !== null) probe.playhead(ms);
    updateHover();
    scheduler.invalidate();
  }

  /** Moves the playhead one pixel (or, coarse, a tenth of the view) and moves the view
   * by the least amount that keeps it in sight (AC-FENCD9.6). */
  export function stepPlayheadBy(direction: -1 | 1, coarse: boolean): void {
    const range = currentRange();
    if (range === null) return;
    const shown = { t0Ms: range[0], t1Ms: range[1] };
    const session = sessionExtent(range[0], range[1]);
    const head = stepPlayhead(playheadMs, direction, shown, plotWidthPx(), coarse, session);
    setPlayhead(head);
    const next = followPlayhead(shown, head, session);
    if (next !== shown) showRange(next);
  }

  /** The J/K/L shuttle's speed, a signed multiple of real time; 0 when stopped
   * (AC-FENCD9.7). */
  let shuttleSpeed = 0;
  /** The pending shuttle animation frame, and the previous frame's timestamp. */
  let shuttleFrame: number | null = null;
  let shuttleLastTs: number | null = null;
  /** The longest step one frame may take, so a stalled tab does not jump the playhead. */
  const SHUTTLE_MAX_DT_MS = 100;

  /** J / K / L: changes the shuttle speed (AC-FENCD9.7). */
  export function shuttle(key: "J" | "K" | "L"): void {
    setShuttle(shuttlePress(shuttleSpeed, key));
  }

  function setShuttle(speed: number): void {
    shuttleSpeed = speed;
    if (E2E_BUILD && probe !== null) probe.shuttle(speed);
    if (speed === 0) {
      if (shuttleFrame !== null) animationFrames.cancel(shuttleFrame);
      shuttleFrame = null;
      shuttleLastTs = null;
      return;
    }
    if (shuttleFrame === null) {
      shuttleLastTs = null;
      shuttleFrame = animationFrames.request(onShuttleFrame);
    }
  }

  /** One shuttle frame: moves the playhead by speed × elapsed time and keeps it in view;
   * playing forwards into the newest data stops and returns to live, playing backwards
   * into the session start stops there. */
  function onShuttleFrame(ts: number): void {
    shuttleFrame = null;
    if (shuttleSpeed === 0) return;
    const dt = shuttleLastTs === null ? 0 : Math.min(SHUTTLE_MAX_DT_MS, ts - shuttleLastTs);
    shuttleLastTs = ts;
    const range = currentRange();
    if (range !== null) {
      const shown = { t0Ms: range[0], t1Ms: range[1] };
      const session = sessionExtent(range[0], range[1]);
      const step = shuttleStep(anchorMs(range), shuttleSpeed, dt, session);
      setPlayhead(step.playheadMs);
      if (step.reachedEnd) {
        setShuttle(0);
        gesture({ kind: "live" });
        return;
      }
      if (step.reachedStart) setShuttle(0);
      // The view follows only once its last move is on screen: moving it every frame
      // would supersede each history answer before it lands (AC-FENCD9.12).
      if (viewSettled()) {
        const next = followPlayhead(shown, step.playheadMs, session);
        if (next !== shown) showRange(next);
      }
    }
    if (shuttleSpeed !== 0) shuttleFrame = animationFrames.request(onShuttleFrame);
  }
  $effect(() => () => setShuttle(0));

  /** The range drawn last came from the history answer for the view, or no answer is
   * coming (live, or the request failed). */
  let drawnFromHistory = false;
  function viewSettled(): boolean {
    if (view.mode !== "history") return true;
    return drawnFromHistory || (requester.idle && historyError !== null);
  }

  /** Session time <-> x display coordinate for the view on screen: gaps longer than a
   * minute squeezed to a narrow break (AC-FENCD9.11). Rebuilt before every draw. */
  let warp: TimeWarp = buildWarp([], { t0Ms: 0, t1Ms: 1 }, 1);
  /** uPlot's default minimum spacing of time-axis ticks, CSS px. */
  const TIME_TICK_SPACE_PX = 50;

  /** The warp for `[t0Ms, t1Ms]` drawn across `widthPx` with this session's gaps; the
   * overview strip builds one for the whole session with it. */
  function warpFor(t0Ms: number, t1Ms: number, widthPx: number): TimeWarp {
    return buildWarp(sessionGaps.overlapping(t0Ms, t1Ms), { t0Ms, t1Ms }, widthPx);
  }

  /** Time-axis ticks every `incr` outside the collapsed gaps, dropping a tick that would
   * crowd the previous one across a break. */
  function warpedSplits(u: uPlot, min: number, max: number, incr: number): number[] {
    const origin = originMs();
    if (warp.breaks.length === 0) return timeSplits(min, max, incr, origin);
    const out: number[] = [];
    let from = min;
    for (const b of warp.breaks) {
      if (b.fromMs > from) out.push(...timeSplits(from, Math.min(b.fromMs, max), incr, origin));
      from = Math.max(from, b.toMs);
      if (from >= max) break;
    }
    if (from < max) out.push(...timeSplits(from, max, incr, origin));
    const d0 = warp.fwd(min);
    const pxPerD = (u.over.clientWidth || 1) / (warp.fwd(max) - d0 || 1);
    const kept: number[] = [];
    let lastPx = -Infinity;
    for (const v of out) {
      const px = (warp.fwd(v) - d0) * pxPerD;
      if (px - lastPx < TIME_TICK_SPACE_PX * 0.75) continue;
      if (kept[kept.length - 1] === v) continue;
      kept.push(v);
      lastPx = px;
    }
    return kept;
  }

  /** The time-axis labels and gap bands of the last draw (for the e2e hook). */
  let drawnTicks: string[] = [];
  let drawnBands: GapBand[] = [];
  /** What the cursor points at (AC-AH2AEM.6), or the playhead while the cursor is off the
   * plot (AC-FENCD9.5); `null` with neither. */
  let hover: HoverReadout | null = $state(null);
  /** Cursor position over the plot in CSS px, so the readout follows a scrolling window. */
  let hoverLeftPx: number | null = null;
  /** Cursor x in ms while it is over the plot. */
  let hoverAtMs: number | null = null;

  /** The pointer's values while it is over the plot, else the playhead's (AC-FENCD9.5). */
  function updateHover(): void {
    const atMs = hoverAtMs ?? playheadMs;
    hover =
      atMs === null || shownFrame === null ? null : hoverReadout(shownFrame, atMs, visibility);
  }

  /** Legend on/off choice, kept for the session (AC-AH2AEM.5). */
  let visibility: SeriesVisibility = $state(loadVisibility(sessionStore()));
  const styles = $derived(seriesStyles(theme));
  const liveBtn = $derived(liveButton(view));
  /** No session: the Live button reads "Live" but is not pressed (AC-9CFMD3.5). */
  const livePressed = $derived(active && liveBtn.pressed);

  function toggle(key: ChartSeriesKey): void {
    visibility = toggleSeries(visibility, key);
    saveVisibility(visibility, sessionStore());
  }

  /** "Zero-based axes": every value axis includes 0, kept for the session (AC-AH2AEM.9). */
  let zeroBased = $state(loadZeroBased(sessionStore()));

  function toggleZeroBased(): void {
    zeroBased = !zeroBased;
    saveZeroBased(zeroBased, sessionStore());
  }

  /** What to draw from now: the history answer, the whole-session answer or the buffer. */
  function source(): "history" | "session" | "live" {
    return frameSource(view, {
      sessionStartMs,
      bufferFirstMs: buffer.firstMs,
      hasHistoryFrame: historyFrame !== null,
      hasSessionFrame: sessionFrame !== null,
    });
  }

  /** The live window's range; reduced motion advances it in tenth-of-a-span steps
   * instead of scrolling. */
  function shownRange(): [number, number] | null {
    const range = liveRange(liveWindow, buffer);
    if (range === null) return null;
    if (liveWindow === "all") {
      return sessionStartMs !== null && source() === "session" ? [sessionStartMs, range[1]] : range;
    }
    const ms = windowMs(liveWindow);
    if (motion().chartScroll === "smooth" || ms === null) return range;
    return steppedRange(range, ms, buffer.firstMs ?? range[0]);
  }

  /** Plot-area width in CSS pixels, read from the DOM (uPlot's bbox can lag a resize). */
  function plotWidthPx(): number {
    return plot ? plot.over.clientWidth : 0;
  }

  /** The x range on screen right now. */
  function currentRange(): [number, number] | null {
    return view.mode === "history" ? [view.t0Ms, view.t1Ms] : shownRange();
  }

  /** An empty frame, drawn before any data. */
  function emptyFrame(): ChartFrame {
    return buffer.window(0, 0, 1);
  }

  /** The frame for `range`: a history answer, or the buffer decimated to the plot width
   * (which also stands in while the first history answer is on its way). */
  function frameFor(range: [number, number] | null): ChartFrame {
    const from = source();
    if (from === "history" && historyFrame !== null) return historyFrame;
    if (from === "session" && sessionFrame !== null) return sessionFrame;
    return range === null ? emptyFrame() : buffer.window(range[0], range[1], plotWidthPx() || 1);
  }

  /** In live mode, asks for the whole session while its start is unknown, or while
   * "Whole session" reaches past the live buffer; at most 4 times a second. */
  function refreshSession(): void {
    const last = buffer.lastMs;
    const first = buffer.firstMs;
    if (view.mode !== "live" || last === null || first === null) return;
    // The overview strip also wants it once the live buffer has moved past its end.
    const sessionEnd = sessionFrame?.x[sessionFrame.x.length - 1];
    const wanted =
      sessionStartMs === null ||
      (liveWindow === "all" && sessionStartMs < first) ||
      (sessionEnd !== undefined && sessionEnd < first);
    if (wanted && sessionGate.ready(performance.now())) {
      requester.request(sessionRequestFor(last, plotWidthPx()));
    }
  }

  function draw(): void {
    const u = plot;
    if (!u) return;
    refreshSession();
    const range = currentRange();
    const frame = frameFor(range);
    shownFrame = frame;
    updateZoomSlider(range);
    warp = range === null ? warpFor(0, 1, 0) : warpFor(range[0], range[1], plotWidthPx());
    u.batch(() => {
      // With no range yet, let uPlot fit the x scale to the (empty) data.
      u.setData(toAlignedData(frame), range === null);
      if (range !== null) u.setScale("x", { min: range[0], max: range[1] });
    });
    const fromHistory =
      frame === historyFrame &&
      view.mode === "history" &&
      historyFrameRange !== null &&
      historyFrameRange[0] === view.t0Ms &&
      historyFrameRange[1] === view.t1Ms;
    drawnFromHistory = fromHistory;
    if (E2E_BUILD && probe !== null) {
      probe.record(frame, {
        mode: view.mode,
        t0Ms: range?.[0] ?? frame.x[0] ?? 0,
        t1Ms: range?.[1] ?? frame.x[frame.x.length - 1] ?? 0,
        fromHistory,
      });
      probe.overlay(drawnTicks, drawnBands);
      const min = u.scales.x?.min;
      const max = u.scales.x?.max;
      probe.breaks(
        min === undefined || max === undefined
          ? []
          : warp.breaks.map((b) => ({
              ...b,
              widthPx:
                u.valToPos(Math.min(b.toMs, max), "x") - u.valToPos(Math.max(b.fromMs, min), "x"),
            })),
      );
    }
    overview?.draw(range === null ? null : overviewState(range), String(sessionStartMs));
    // The cursor stays put while the window scrolls: read its time on the new scale.
    if (hoverLeftPx !== null) hoverAtMs = u.posToVal(hoverLeftPx, "x");
    updateHover();
  }

  /** The overview strip under the plot (AC-FENCD9.1). */
  let overview: OverviewStrip | undefined = $state();

  /** What the overview strip shows for `range`. */
  function overviewState(range: [number, number]): OverviewState {
    return {
      range: { t0Ms: range[0], t1Ms: range[1] },
      session: sessionExtent(range[0], range[1]),
      originMs: originMs(),
    };
  }

  /** The strip's envelope: the history answer up to where the live buffer starts, the
   * buffer after it. */
  function envelopeSources(cols: number): EnvelopeSource[] {
    const out: EnvelopeSource[] = [];
    const bufFirst = buffer.firstMs;
    const bufLast = buffer.lastMs;
    if (sessionFrame !== null) out.push({ frame: sessionFrame, beforeMs: bufFirst ?? Infinity });
    if (bufFirst !== null && bufLast !== null)
      out.push({ frame: buffer.window(bufFirst, bufLast, cols), beforeMs: Infinity });
    return out;
  }

  /** Something has been drawn, so the Live button has a range to pause (AC-DDDYDJ.2). */
  let hasData = $state(false);

  /** Test builds only: what the e2e hook reports (FR AH2AEN). */
  const probe: ChartProbe | null = E2E_BUILD ? new ChartProbe() : null;
  if (E2E_BUILD && probe !== null) $effect(() => installChartProbe(probe));

  const scheduler = new RedrawScheduler(draw);
  $effect(() => () => scheduler.stop());

  /** A batch arrived. */
  export function push(samples: readonly Sample[]): void {
    buffer.push(samples);
    if (!hasData && buffer.length > 0) hasData = true;
    if (backfillArmed) startBackfill();
    scheduler.invalidate();
  }

  /** A re-attach adopted a running session: once the buffer has its first sample, fill
   * it from one history query covering the longest live window before it (FR VEDDFD). */
  let backfillArmed = false;
  /** Bumped by {@link reset}, so a backfill answer for an earlier session is dropped. */
  let backfillGeneration = 0;

  /** Backfills the live buffer from the history once it has a sample (FR VEDDFD). */
  export function backfillFromHistory(): void {
    backfillArmed = true;
    startBackfill();
  }

  function startBackfill(): void {
    const first = buffer.firstMs;
    if (!backfillArmed || first === null) return;
    // Once per re-attach: a session whose first live sample is at its start has
    // nothing before it to fill.
    backfillArmed = false;
    const generation = backfillGeneration;
    const [t0Ms, t1Ms] = backfillRange(first);
    if (!(t1Ms > t0Ms)) return;
    fetchBackfill(first, { historyRange, historyGaps })
      .then(({ samples, gaps }) => {
        if (generation !== backfillGeneration) return;
        buffer.prepend(samples);
        // After the prepend, so each break sits between two buffered samples.
        for (const g of gaps) {
          buffer.markGap(g);
          sessionGaps.push(g);
        }
        scheduler.invalidate();
      })
      .catch((e: unknown) => console.warn("open-powerz: backfill failed", e));
  }

  /** The stream stopped for a while: the lines break there, never bridge it. */
  export function markGap(gap: Gap): void {
    buffer.markGap(gap);
    sessionGaps.push(gap);
    scheduler.invalidate();
  }

  /** A new session starts: forget the previous one's samples, gaps, start, playhead
   * and hover, and go back to the live window. */
  export function reset(): void {
    cancelAnnouncement();
    hoverLeftPx = null;
    hoverAtMs = null;
    setPlayhead(null);
    buffer.clear();
    backfillArmed = false;
    backfillGeneration++;
    historyGeneration++;
    hasData = false;
    sessionGaps.clear();
    sessionStartMs = null;
    sessionFrame = null;
    sessionGate.reset();
    view = { mode: "live", window: liveWindow };
    setShuttle(0);
    leaveHistory();
    zoomText = "";
    scheduler.invalidate();
  }

  /** Bumped by {@link reset}, so gaps from a query sent before it are dropped. */
  let historyGeneration = 0;

  /** The backend history was cleared (FR 5X154V): drop the live buffer, the session start,
   * the whole-session frame, the gaps, the overview envelope and any history answer in
   * flight, and go back to the live window. The stream keeps feeding `push`. */
  export function clearHistory(): void {
    const wasLive = view.mode === "live";
    reset();
    cancelAnnouncement();
    announcement = wasLive ? "History cleared" : "History cleared, back to live";
    // Draw the emptied chart now, not on a coming frame: nothing from before the clear
    // stays on screen while the next batch is on its way.
    draw();
  }

  /** One `history_range` call in flight at most; the newest view wins (AC-AH2AEM.4). */
  const requester = new HistoryRequester({
    // The history also says where its runs break (an unplug, a disconnect before this
    // connection), which no stream event reports (AC-272WSN.3, AC-272WSN.4).
    fetch: async (req) => {
      const generation = historyGeneration;
      const [buckets, gaps] = await Promise.all([
        historyRange(req.series, req.t0Ms, req.t1Ms, req.buckets),
        // A failed gap query leaves the gaps already known; the buckets still draw.
        historyGaps(req.t0Ms, req.t1Ms).catch(() => []),
      ]);
      if (generation === historyGeneration) for (const g of gaps) sessionGaps.push(g);
      return buckets;
    },
    onResult: (req, result) => {
      const gaps = sessionGaps.within(req.t0Ms, req.t1Ms);
      if (req.purpose === "session") {
        sessionStartMs = sessionStartOf(result) ?? sessionStartMs;
        sessionFrame = bucketsToFrame(result, gaps);
        scheduler.invalidate();
        return;
      }
      if (view.mode !== "history") return;
      historyError = null;
      historyFrame = bucketsToFrame(result, gaps);
      historyFrameRange = [req.t0Ms, req.t1Ms];
      // A zoom that came before the session start was known still needs it for clamping.
      const last = buffer.lastMs;
      if (sessionStartMs === null && last !== null) {
        requester.request(sessionRequestFor(last, plotWidthPx()));
      }
      scheduler.invalidate();
    },
    onError: (e) => {
      if (view.mode !== "history") return;
      const message = (e as { message?: unknown } | null)?.message;
      historyError = typeof message === "string" ? message : "The history could not be read.";
    },
  });
  $effect(() => () => requester.cancel());

  /** Asks the backend for the history view's buckets, one per pixel, and draws them. */
  function fetchHistory(): void {
    const req = historyRequestFor(view, plotWidthPx());
    if (req !== null) requester.request(req);
  }

  /** Back to live: drop any history request and frame. */
  function leaveHistory(): void {
    requester.cancel();
    historyFrame = null;
    historyFrameRange = null;
    historyError = null;
  }

  /** Applies a zoom / pan / live gesture, kept inside the session's extent. */
  function gesture(g: ChartGesture): void {
    const range = currentRange();
    if (range === null && g.kind !== "live") return;
    const [t0Ms, t1Ms] = range ?? [0, 0];
    const session = sessionExtent(t0Ms, t1Ms);
    const next = applyGesture(view, g, { t0Ms, t1Ms }, session, warpFor(t0Ms, t1Ms, plotWidthPx()));
    if (next === view) return;
    view = next;
    drawnFromHistory = false;
    if (view.mode === "history") {
      fetchHistory();
    } else {
      setShuttle(0);
      leaveHistory();
    }
    announceView();
    scheduler.invalidate();
  }

  /** Home key: paused, same span, from the session start. */
  export function goHome(): void {
    gesture({ kind: "home" });
  }

  /** + / = / −: zooms in (or out) around the playhead, or the view centre without one
   * (AC-FENCD9.8). */
  export function zoomStep(direction: "in" | "out"): void {
    const range = currentRange();
    if (range === null) return;
    gesture({ kind: "zoom", factor: direction === "in" ? 0.8 : 1 / 0.8, atMs: anchorMs(range) });
  }

  /** Shift+Z: the whole session, live (the "all" window) (AC-FENCD9.8). */
  export function fitSession(): void {
    gesture({ kind: "fit" });
    if (view.mode === "live" && liveWindow !== view.window) {
      chooseLiveWindow(view.window);
      sessionGate.reset();
    }
  }

  /** End key: back to the scrolling live window (the Live button toggles via `liveToggle`). */
  export function returnToLive(): void {
    gesture({ kind: "live" });
  }

  /** Set the live window and keep the choice for the session (AC-AKNVND.5). */
  function chooseLiveWindow(id: LiveWindowId): void {
    liveWindow = id;
    saveLiveWindow(id, sessionStore());
  }

  /** A window button: live, that scrolling window; paused, that span around the playhead
   * (or the view centre), still paused (AC-FENCD9.10). */
  function pickWindow(id: LiveWindowId): void {
    if (view.mode === "history") {
      const range = currentRange();
      if (range === null) return;
      const session = sessionExtent(range[0], range[1]);
      const next = presetRange(windowMs(id), anchorMs(range), session);
      chooseLiveWindow(id);
      showRange(next);
      if (view.mode === "history") view = { ...view, window: id };
      return;
    }
    chooseLiveWindow(id);
    view = { mode: "live", window: id };
    leaveHistory();
    sessionGate.reset();
    announceView();
  }

  /** A window button is pressed while live on that window, or paused on that preset's
   * span (Whole: the whole session) (AC-FENCD9.10). */
  function presetPressed(v: ChartView, id: LiveWindowId): boolean {
    if (v.mode === "live") return windowPressed(v, id);
    if (v.window !== id) return false;
    const session = sessionExtent(v.t0Ms, v.t1Ms);
    const want = Math.min(windowMs(id) ?? Infinity, session.lastMs - session.firstMs);
    return Math.abs(v.t1Ms - v.t0Ms - want) <= 1;
  }

  function onKeydown(e: KeyboardEvent): void {
    if (e.ctrlKey || e.metaKey || e.altKey || !CHART_KEYS.includes(e.key)) return;
    e.preventDefault();
    // ← / → step the playhead here; the app-wide shortcut must not step it again.
    if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
      e.stopPropagation();
      stepPlayheadBy(e.key === "ArrowLeft" ? -1 : 1, e.shiftKey);
      return;
    }
    // End is also an app-wide shortcut; the chart has handled it, so it stops here.
    if (e.key === "End") e.stopPropagation();
    gesture({ kind: "key", key: e.key });
  }

  /** Device pixel ratio the plot is drawn at: line widths and dashes snap to it
   * (AC-AKNVND.1), so a change (window moved to another screen, zoom) rebuilds the plot. */
  let pxRatio = $state(globalThis.devicePixelRatio || 1);
  $effect(() => {
    if (typeof matchMedia !== "function") return;
    const query = matchMedia(`(resolution: ${pxRatio}dppx)`);
    const onChange = (): void => {
      pxRatio = globalThis.devicePixelRatio || 1;
    };
    query.addEventListener("change", onChange);
    return () => query.removeEventListener("change", onChange);
  });

  /** The host's size in CSS pixels, never zero (uPlot needs a positive size). */
  function sizeOf(el: HTMLElement): { width: number; height: number } {
    return { width: Math.max(1, el.clientWidth), height: Math.max(1, el.clientHeight) };
  }

  // (Re)build the plot when the host appears, the theme changes, a series is toggled
  // (uPlot cannot show / hide an axis in place) or the axes switch between auto-scale
  // and zero-based; a resize only resizes it.
  $effect(() => {
    const el = host;
    if (!el) return;
    const opts = chartOptions({
      theme,
      visibility,
      zeroBased,
      originMs,
      pxRatio,
      ...sizeOf(el),
    });
    const timeAxis = opts.axes?.[0];
    // Long gaps collapse to a narrow break: x is drawn through the warp (AC-FENCD9.11).
    opts.scales = {
      ...opts.scales,
      x: {
        ...opts.scales?.x,
        distr: 100,
        fwd: (v: number) => warp.fwd(v),
        bwd: (v: number) => warp.bwd(v),
      },
    };
    if (timeAxis) {
      // Tick spacing follows the warped span, so the data beside a break gets its own
      // ticks; none falls inside a collapsed gap.
      timeAxis.space = (_u: uPlot, _i: number, min: number, max: number) =>
        max > min
          ? (TIME_TICK_SPACE_PX * (warp.fwd(max) - warp.fwd(min))) / (max - min)
          : TIME_TICK_SPACE_PX;
      timeAxis.splits = (self: uPlot, _i: number, min: number, max: number, incr: number) =>
        warpedSplits(self, min, max, incr);
      // uPlot's default filter treats any distr >= 3 as logarithmic and blanks the
      // ticks; a warped time axis keeps them all.
      timeAxis.filter = (_u: uPlot, splits: number[]) => splits;
    }
    const timeValues = timeAxis?.values;
    if (timeAxis && typeof timeValues === "function") {
      timeAxis.values = (...args) => {
        const labels = timeValues(...args);
        drawnTicks = labels.map(String);
        return labels;
      };
    }
    const palette = THEME_COLORS[theme];
    opts.hooks = {
      ...opts.hooks,
      // Stretches without data are shaded under the lines (drawClear) and say how long
      // they last above the grid (draw), so no grid line crosses the label (AC-272WSN.5).
      drawClear: [
        ...(opts.hooks?.drawClear ?? []),
        (self: uPlot) => {
          const min = self.scales.x?.min;
          const max = self.scales.x?.max;
          drawnBands =
            min === undefined || max === undefined ? [] : gapBands(sessionGaps, min, max);
          if (drawnBands.length === 0) return;
          const ctx = self.ctx;
          const { top, height } = self.bbox;
          ctx.save();
          ctx.globalAlpha = 0.14;
          ctx.fillStyle = palette.muted;
          for (const band of drawnBands) {
            const x0 = self.valToPos(band.fromMs, "x", true);
            const x1 = self.valToPos(band.toMs, "x", true);
            ctx.fillRect(x0, top, Math.max(1, x1 - x0), height);
          }
          ctx.restore();
        },
      ],
      draw: [
        ...(opts.hooks?.draw ?? []),
        (self: uPlot) => {
          if (drawnBands.length === 0) return;
          const ctx = self.ctx;
          const { top } = self.bbox;
          const ratio = globalThis.devicePixelRatio || 1;
          ctx.save();
          ctx.font = `${Math.round(11 * ratio)}px system-ui, sans-serif`;
          ctx.textAlign = "center";
          ctx.textBaseline = "top";
          ctx.fillStyle = palette.muted;
          // Bands and breaks are both in time order: one walk finds each band's break.
          const breaks = warp.breaks;
          let bi = 0;
          for (const band of drawnBands) {
            while (bi < breaks.length && breaks[bi]!.toMs <= band.fromMs) bi++;
            const x0 = self.valToPos(band.fromMs, "x", true);
            const x1 = self.valToPos(band.toMs, "x", true);
            // Narrower than a pixel: no label of any length fits.
            if (x1 - x0 < ratio) continue;
            const brk = breaks[bi];
            const collapsed = brk !== undefined && brk.fromMs < band.toMs;
            // Most bands of a long session are too narrow for any label: skip measuring them.
            const wide = x1 - x0 >= 40 * ratio;
            const label =
              wide && gapLabelFits(ctx.measureText(band.label).width, x1 - x0, ratio)
                ? band.label
                : // A collapsed gap too narrow for its label still says how long it is, in
                  // short form, over the break (AC-FENCD9.11).
                  collapsed
                  ? (band.label.split(" · ").pop() ?? band.label)
                  : null;
            if (label === null) continue;
            // A label is drawn only where it fits inside the plot, never over an axis
            // (a break cut by the edge of the view keeps its band, not its text).
            const half = ctx.measureText(label).width / 2;
            const mid = (x0 + x1) / 2;
            const { left, width } = self.bbox;
            if (mid - half < left || mid + half > left + width) continue;
            ctx.fillText(label, mid, top + 8 * ratio);
          }
          ctx.restore();
        },
        // Each collapsed gap is marked on the time axis with a pair of slanted strokes.
        (self: uPlot) => {
          if (warp.breaks.length === 0) return;
          const min = self.scales.x?.min;
          const max = self.scales.x?.max;
          if (min === undefined || max === undefined) return;
          const ratio = globalThis.devicePixelRatio || 1;
          const { top, height } = self.bbox;
          const y = top + height;
          const h = 5 * ratio;
          const ctx = self.ctx;
          ctx.save();
          ctx.strokeStyle = palette.muted;
          ctx.lineWidth = Math.max(1, ratio);
          ctx.beginPath();
          for (const b of warp.breaks) {
            for (const t of [b.fromMs, b.toMs]) {
              if (t <= min || t >= max) continue;
              const x = self.valToPos(t, "x", true);
              ctx.moveTo(x - h / 2, y + h);
              ctx.lineTo(x + h / 2, y - h);
            }
          }
          ctx.stroke();
          ctx.restore();
        },
        // The playhead: a vertical line across the plot (AC-FENCD9.5).
        (self: uPlot) => {
          if (playheadMs === null) return;
          const min = self.scales.x?.min;
          const max = self.scales.x?.max;
          if (min === undefined || max === undefined) return;
          if (playheadMs < min || playheadMs > max) return;
          const x = Math.round(self.valToPos(playheadMs, "x", true));
          const { top, height } = self.bbox;
          const ctx = self.ctx;
          ctx.save();
          ctx.strokeStyle = palette.text;
          ctx.lineWidth = Math.max(1, globalThis.devicePixelRatio || 1);
          ctx.beginPath();
          ctx.moveTo(x, top);
          ctx.lineTo(x, top + height);
          ctx.stroke();
          ctx.restore();
        },
      ],
      setCursor: [
        ...(opts.hooks?.setCursor ?? []),
        (self: uPlot) => {
          const left = self.cursor.left;
          hoverLeftPx = left === undefined || left < 0 ? null : left;
          hoverAtMs = hoverLeftPx === null ? null : self.posToVal(hoverLeftPx, "x");
          updateHover();
        },
      ],
      // The plot area now has its real width: a history view asks again with one
      // bucket per pixel (also the first ask, when it was entered at width 0).
      setSize: [
        ...(opts.hooks?.setSize ?? []),
        () =>
          untrack(() => {
            if (view.mode === "history") fetchHistory();
            scheduler.invalidate();
          }),
      ],
    };
    const u = new uPlot(opts, toAlignedData(emptyFrame()), el);
    plot = u;
    const over = u.over;
    /** The plot x under a viewport x, clamped to the plot. */
    const plotX = (clientX: number): number => {
      const rect = over.getBoundingClientRect();
      return Math.min(Math.max(clientX - rect.left, 0), rect.width);
    };
    /** The time under a viewport x, clamped to the plot. */
    const timeAtClientX = (clientX: number): number => u.posToVal(plotX(clientX), "x");

    // Over the plot the wheel (and a trackpad pinch, sent as Ctrl+wheel) zooms around
    // the pointer; Shift+wheel and a horizontal scroll pan (AC-FENCD9.3).
    const onWheel = (e: WheelEvent): void => {
      e.preventDefault();
      gesture({
        kind: "wheel",
        // By viewport x: the event may land on one of uPlot's cursor elements.
        atMs: timeAtClientX(e.clientX),
        deltaY: e.deltaY,
        deltaX: e.deltaX,
        shiftKey: e.shiftKey,
      });
    };

    // Shift-drag or middle-button drag pans, a plain drag scrubs the playhead and an
    // Alt-drag zooms to the dragged range (AC-FENCD9.4); all three are captured before
    // uPlot sees them, so its own drag-select never starts.
    let panFromX: number | null = null;
    let scrubbing = false;
    /** An Alt-drag in progress: the plot x it started at (handled here rather than by
     * uPlot's own drag-select, which WebKit never reaches with Alt held). */
    let selectFromX: number | null = null;
    const onDragMove = (e: MouseEvent): void => {
      // No button held: the mouseup went missing (released outside the window), so
      // the drag ends here, nothing applied.
      if (e.buttons === 0) {
        onDragEnd();
        return;
      }
      if (selectFromX !== null) {
        const x = plotX(e.clientX);
        const left = Math.min(selectFromX, x);
        u.setSelect(
          { left, top: 0, width: Math.abs(x - selectFromX), height: over.clientHeight },
          false,
        );
        return;
      }
      if (scrubbing) {
        setPlayhead(timeAtClientX(e.clientX));
        return;
      }
      if (panFromX === null) return;
      const width = over.clientWidth || 1;
      const range = currentRange();
      if (range === null) return;
      const deltaMs = (-(e.clientX - panFromX) / width) * (range[1] - range[0]);
      panFromX = e.clientX;
      if (deltaMs !== 0) gesture({ kind: "pan", deltaMs });
    };
    const onDragEnd = (e?: MouseEvent): void => {
      if (selectFromX !== null) {
        const from = selectFromX;
        selectFromX = null;
        u.setSelect({ left: 0, top: 0, width: 0, height: 0 }, false);
        const to = e ? plotX(e.clientX) : from;
        if (Math.abs(to - from) > 0) {
          const t0Ms = u.posToVal(Math.min(from, to), "x");
          const t1Ms = u.posToVal(Math.max(from, to), "x");
          gesture({ kind: "select", t0Ms, t1Ms });
        }
      }
      panFromX = null;
      scrubbing = false;
      window.removeEventListener("mousemove", onDragMove);
      window.removeEventListener("mouseup", onDragEnd);
      window.removeEventListener("blur", onDragCancel);
    };
    /** The window lost focus mid-drag: its mouseup may never come, so cancel. */
    const onDragCancel = (): void => onDragEnd();
    const onDragStart = (e: MouseEvent): void => {
      if (!over.contains(e.target as Node)) return;
      const pan = e.button === 1 || (e.button === 0 && e.shiftKey);
      const select = e.button === 0 && !e.shiftKey && e.altKey;
      const scrub = e.button === 0 && !e.shiftKey && !e.altKey;
      if (!pan && !scrub && !select) return;
      e.preventDefault();
      e.stopPropagation();
      // preventDefault keeps focus where it was: land the keyboard on the plot, without
      // a focus ring (the focus came from the mouse).
      pointerFocus = true;
      el.focus({ preventScroll: true });
      if (pan) panFromX = e.clientX;
      else if (select) selectFromX = plotX(e.clientX);
      else {
        scrubbing = true;
        setPlayhead(timeAtClientX(e.clientX));
      }
      window.addEventListener("mousemove", onDragMove);
      window.addEventListener("mouseup", onDragEnd);
      window.addEventListener("blur", onDragCancel);
    };
    // A double-click fits the whole session.
    const onDblClick = (e: MouseEvent): void => {
      e.preventDefault();
      fitSession();
    };
    over.addEventListener("wheel", onWheel, { passive: false });
    over.addEventListener("dblclick", onDblClick);
    el.addEventListener("mousedown", onDragStart, true);

    // Resize the plot on the next frame, not inside the observation (a resize there makes
    // WebKit log a ResizeObserver loop error). One pending frame at a time.
    let resizeFrame = 0;
    const observer = new ResizeObserver(() => {
      if (resizeFrame) return;
      resizeFrame = requestAnimationFrame(() => {
        resizeFrame = 0;
        u.setSize(sizeOf(el));
        scheduler.invalidate();
      });
    });
    observer.observe(el);
    // Draw now, so a rebuilt plot (legend toggle, theme change) never flashes empty;
    // untracked, so drawing does not make this effect depend on the view.
    untrack(draw);
    return () => {
      over.removeEventListener("wheel", onWheel);
      over.removeEventListener("dblclick", onDblClick);
      el.removeEventListener("mousedown", onDragStart, true);
      onDragEnd();
      observer.disconnect();
      if (resizeFrame) cancelAnimationFrame(resizeFrame);
      u.destroy();
      if (plot === u) plot = null;
    };
  });

  // A new window choice or motion setting redraws.
  $effect(() => {
    void liveWindow;
    void motion().chartScroll;
    scheduler.invalidate();
  });
</script>

<div class="chart-wrap">
  <div class="toolbar">
    <div class="windows" role="group" aria-label="Live window">
      {#each LIVE_WINDOWS as w (w.id)}
        <button
          type="button"
          aria-pressed={presetPressed(view, w.id)}
          class:primary={presetPressed(view, w.id)}
          onclick={() => pickWindow(w.id)}>{w.label}</button
        >
      {/each}
    </div>
    <button
      type="button"
      class="zero-based"
      class:primary={zeroBased}
      aria-pressed={zeroBased}
      title="Start every value axis at zero, so small changes are drawn to scale"
      onclick={toggleZeroBased}>Zero-based axes</button
    >
    <input
      type="range"
      class="zoom-slider"
      aria-label="Zoom"
      aria-valuetext={zoomText}
      title="Zoom: visible span"
      min="0"
      max={ZOOM_SLIDER_MAX}
      step="1"
      value={zoomValue}
      disabled={!hasData}
      oninput={onZoomInput}
    />
    <!-- The span the slider sets, in the time axis's own format; screen readers hear it
         as the slider's value text. -->
    <span class="zoom-span" title="Zoom: visible span" aria-hidden="true"
      >{active ? zoomText : ""}</span
    >
    <button
      type="button"
      class="live"
      class:primary={livePressed}
      class:paused={active && !liveBtn.pressed}
      aria-pressed={livePressed}
      disabled={view.mode === "live" && !hasData}
      title={liveBtn.title}
      onclick={() => gesture(liveToggle(view))}>{liveBtn.label}</button
    >
  </div>
  <div class="legend" role="group" aria-label="Series">
    {#each CHART_SERIES as s, i (s.key)}
      <button
        type="button"
        class="series"
        aria-pressed={visibility[s.key]}
        title={`Show or hide ${s.label}`}
        onclick={() => toggle(s.key)}
      >
        <svg class="swatch" width="24" height="8" aria-hidden="true">
          <line
            x1="0"
            y1="4"
            x2="24"
            y2="4"
            stroke={styles[i]?.stroke}
            stroke-width="2"
            stroke-dasharray={styles[i]?.dash.join(" ") || undefined}
          />
        </svg>
        {s.label} <span class="unit">({s.unit})</span>
      </button>
    {/each}
  </div>
  <p class="visually-hidden" role="status" aria-live="polite">{announcement}</p>
  <!-- Hover readout: a strip above the plot so it never covers the cursor point. -->
  <div class="hover" aria-live="off">
    {#if hover !== null}
      <span class="hover-time">{formatSessionTime(hover.t_ms - originMs(), 3)}</span>
      {#each hover.values as v (v.key)}
        <span class="hover-value">
          <span class="hover-label">{seriesDef(v.key)?.label ?? v.key}</span>
          {formatHoverValue(v)}
        </span>
      {/each}
    {/if}
  </div>
  <!-- The plot is a focusable keyboard-driven widget (arrow keys pan / zoom), which
       svelte treats `role="application"` as non-interactive for. -->
  <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
  <div
    class="plot"
    class:pointer-focus={pointerFocus}
    bind:this={host}
    role="application"
    tabindex="0"
    aria-label="Chart. Click or drag to move the playhead, Alt-drag to zoom to a range, middle-drag or Shift-drag to pan, double-click to fit the session; the wheel or a pinch zooms, Shift-wheel or a horizontal scroll pans; left and right arrows step the playhead, up and down arrows zoom, J, K and L shuttle the playhead, Home jumps to the session start, End returns to live."
    onkeydown={onKeydown}
    onblur={() => (pointerFocus = false)}
  >
    {#if !active}
      {#if hasData}
        <!-- After a disconnect the last session stays on screen to look back at. -->
        <p class="empty-state banner">
          Meter disconnected, showing the last session. Connect a meter to start a new one.
        </p>
      {:else}
        <p class="empty-state">Connect a meter to start</p>
      {/if}
    {/if}
    {#if historyError !== null && view.mode === "history"}
      <p class="history-error" role="status">{historyError}</p>
    {/if}
  </div>
  <!-- Overview strip: the whole session, with a window over the range on screen. -->
  <OverviewStrip
    bind:this={overview}
    {theme}
    current={() => {
      const range = currentRange();
      return range === null ? null : overviewState(range);
    }}
    {warpFor}
    gapsVersion={() => sessionGaps.version}
    sources={envelopeSources}
    onrange={showRange}
    onresize={() => scheduler.invalidate()}
    onbox={(box) => {
      if (E2E_BUILD && probe !== null) probe.overview(box);
    }}
    onbreaks={(breaks) => {
      if (E2E_BUILD && probe !== null) probe.overviewBreaks(breaks);
    }}
  />
</div>

<style>
  .chart-wrap {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    height: 100%;
    min-width: 0;
    min-height: 0;
  }
  .toolbar,
  .windows {
    display: flex;
    gap: 0.25rem;
    flex-wrap: wrap;
  }
  .zero-based {
    margin-left: auto;
  }
  .zoom-slider {
    width: 8rem;
    align-self: center;
  }
  .zoom-span {
    align-self: center;
    min-width: 3.5em;
    margin-left: -0.125rem;
    font-size: 0.85em;
    font-variant-numeric: tabular-nums;
    color: var(--muted, GrayText);
  }
  /* Paused: the accent colour says the chart is not following the stream (AC-DDDYDJ.4). */
  .live.paused {
    border-color: var(--accent);
    color: var(--accent);
    box-shadow: inset 0 0 0 1px var(--accent);
  }
  .legend {
    display: flex;
    gap: 0.25rem;
    flex-wrap: wrap;
  }
  .series {
    display: inline-flex;
    align-items: center;
    gap: 0.375rem;
  }
  /* Off state: muted text (still ≥ 4.5:1), struck through, faded swatch. */
  .series[aria-pressed="false"] {
    color: var(--muted);
    border-style: dashed;
    text-decoration: line-through;
  }
  .series[aria-pressed="false"] .swatch {
    opacity: 0.35;
  }
  .unit {
    color: var(--muted);
  }
  .hover {
    display: flex;
    flex-wrap: wrap;
    gap: 0.25rem 0.875rem;
    min-height: 1.4em;
    font-size: 0.85em;
    font-variant-numeric: tabular-nums;
    color: var(--text, CanvasText);
  }
  /* Below the supported 900 px minimum the readout keeps to one line: tighter, and
     clipped at the edge rather than wrapping under itself. */
  @media (max-width: 700px) {
    .hover {
      flex-wrap: nowrap;
      gap: 0.5rem;
      overflow: hidden;
      white-space: nowrap;
      font-size: 0.8em;
    }
  }
  .hover-time,
  .hover-label {
    color: var(--muted, GrayText);
  }
  .hover-label {
    margin-right: 0.25rem;
  }
  /* Keyboard focus only: a focus the mouse gave shows no ring until focus leaves (this
     also overrides the app-wide :focus-visible ring keys would otherwise bring back). */
  .plot.pointer-focus:focus-visible {
    outline: none;
  }
  .plot:focus-visible:not(.pointer-focus) {
    outline: 2px solid currentColor;
    outline-offset: 2px;
  }
  .history-error {
    position: absolute;
    top: 0.25rem;
    right: 0.5rem;
    z-index: 1;
    margin: 0;
    padding: 0.125rem 0.5rem;
    border-radius: 0.25rem;
    background: var(--surface, Canvas);
    color: var(--danger, CanvasText);
    font-size: 0.85em;
    pointer-events: none;
  }
  .empty-state {
    position: absolute;
    inset: 0;
    z-index: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    margin: 0;
    background: var(--surface, Canvas);
    color: var(--muted, GrayText);
    pointer-events: none;
  }
  .empty-state.banner {
    inset: 0 0 auto 0;
    padding: 0.35rem 0.75rem;
    background: color-mix(in srgb, var(--surface, Canvas) 85%, transparent);
    border-bottom: 1px solid var(--border, GrayText);
    font-size: 0.9em;
  }
  .plot {
    flex: 1;
    min-width: 0;
    min-height: 12rem;
    position: relative;
    overflow: hidden;
  }
</style>
