<script lang="ts">
  import uPlot from "uplot";
  import "uplot/dist/uPlot.min.css";
  import { getContext, untrack } from "svelte";
  import { historyRange, type Gap, type Sample } from "../backend";
  import {
    GapList,
    LIVE_CAPACITY,
    LIVE_WINDOWS,
    LiveBuffer,
    bucketsToFrame,
    hoverReadout,
    liveRange,
    steppedRange,
    toAlignedData,
    type ChartFrame,
    type HoverReadout,
    type HoverValue,
    type LiveWindowId,
  } from "../chartData";
  import {
    CHART_SERIES,
    chartOptions,
    formatSessionSeconds,
    loadVisibility,
    loadZeroBased,
    saveVisibility,
    saveZeroBased,
    seriesDef,
    seriesStyles,
    sessionStore,
    toggleSeries,
    type ChartSeriesKey,
    type SeriesVisibility,
  } from "../chartSeries";
  import {
    CHART_KEYS,
    HistoryRequester,
    RateGate,
    RedrawScheduler,
    applyGesture,
    frameSource,
    historyRequestFor,
    rangeAnnouncement,
    sessionRequestFor,
    sessionStartOf,
    wheelZooms,
    windowPressed,
    type ChartGesture,
    type ChartView,
  } from "../chartView";
  import { E2E_BUILD, ChartProbe, installChartProbe } from "../e2eHook";
  import { formatSigned, formatSignedNumber } from "../stats";
  import type { MotionSettings, ResolvedTheme } from "../theme";

  let { theme }: { theme: ResolvedTheme } = $props();

  const motion = getContext<() => MotionSettings>("motion");

  /** The live samples; not reactive, the scheduler redraws when it changes. */
  const buffer = new LiveBuffer(LIVE_CAPACITY);
  /** This session's gaps, so history frames break where the stream did (AC-AH2AEM.7). */
  const sessionGaps = new GapList();
  let liveWindow: LiveWindowId = $state("10s");
  /** Live (scrolling) or a fixed history range picked by zoom / pan (AC-AH2AEM.3). */
  let view: ChartView = $state({ mode: "live", window: "10s" });
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
  /** What the cursor points at (AC-AH2AEM.6); `null` while it is off the plot. */
  let hover: HoverReadout | null = $state(null);
  /** Cursor position over the plot in CSS px, so the readout follows a scrolling window. */
  let hoverLeftPx: number | null = null;
  /** Cursor x in ms while it is over the plot. */
  let hoverAtMs: number | null = null;

  function updateHover(): void {
    hover =
      hoverAtMs === null || shownFrame === null
        ? null
        : hoverReadout(shownFrame, hoverAtMs, visibility);
  }

  /** `5.500 V`, or `4.800–5.200 V` for a decimated bucket. */
  function hoverText(v: HoverValue): string {
    const unit = seriesDef(v.key)?.unit ?? "";
    if (v.kind === "value") return formatSigned(v.value, 3, unit);
    return `${formatSignedNumber(v.min, 3)}–${formatSignedNumber(v.max, 3)} ${unit}`;
  }

  /** Legend on/off choice, kept for the session (AC-AH2AEM.5). */
  let visibility: SeriesVisibility = $state(loadVisibility(sessionStore()));
  const styles = $derived(seriesStyles(theme));

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
    const ms = LIVE_WINDOWS.find((w) => w.id === liveWindow)?.ms ?? null;
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
    const wanted = sessionStartMs === null || (liveWindow === "all" && sessionStartMs < first);
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
    u.batch(() => {
      // With no range yet, let uPlot fit the x scale to the (empty) data.
      u.setData(toAlignedData(frame), range === null);
      if (range !== null) u.setScale("x", { min: range[0], max: range[1] });
    });
    if (E2E_BUILD && probe !== null) {
      const fromHistory =
        frame === historyFrame &&
        view.mode === "history" &&
        historyFrameRange !== null &&
        historyFrameRange[0] === view.t0Ms &&
        historyFrameRange[1] === view.t1Ms;
      probe.record(frame, {
        mode: view.mode,
        t0Ms: range?.[0] ?? frame.x[0] ?? 0,
        t1Ms: range?.[1] ?? frame.x[frame.x.length - 1] ?? 0,
        fromHistory,
      });
    }
    // The cursor stays put while the window scrolls: read its time on the new scale.
    if (hoverLeftPx !== null) hoverAtMs = u.posToVal(hoverLeftPx, "x");
    updateHover();
  }

  /** Test builds only: what the e2e hook reports (FR AH2AEN). */
  const probe: ChartProbe | null = E2E_BUILD ? new ChartProbe() : null;
  if (E2E_BUILD && probe !== null) $effect(() => installChartProbe(probe));

  const scheduler = new RedrawScheduler(draw);
  $effect(() => () => scheduler.stop());

  /** A batch arrived. */
  export function push(samples: readonly Sample[]): void {
    buffer.push(samples);
    scheduler.invalidate();
  }

  /** The stream stopped for a while: the lines break there, never bridge it. */
  export function markGap(gap: Gap): void {
    buffer.markGap(gap);
    sessionGaps.push(gap);
    scheduler.invalidate();
  }

  /** A new session starts: forget the previous one's samples, gaps and start, and go
   * back to the live window. */
  export function reset(): void {
    buffer.clear();
    sessionGaps.clear();
    sessionStartMs = null;
    sessionFrame = null;
    sessionGate.reset();
    view = { mode: "live", window: liveWindow };
    leaveHistory();
    scheduler.invalidate();
  }

  /** One `history_range` call in flight at most; the newest view wins (AC-AH2AEM.4). */
  const requester = new HistoryRequester({
    fetch: (req) => historyRange(req.series, req.t0Ms, req.t1Ms, req.buckets),
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
    const session = {
      firstMs: sessionStartMs ?? buffer.firstMs ?? t0Ms,
      lastMs: Math.max(buffer.lastMs ?? t1Ms, t1Ms),
    };
    const next = applyGesture(view, g, { t0Ms, t1Ms }, session);
    if (next === view) return;
    const wasLive = view.mode === "live";
    view = next;
    if (view.mode === "history") {
      fetchHistory();
      if (wasLive) announcement = rangeAnnouncement(view);
    } else {
      leaveHistory();
      if (!wasLive) announcement = rangeAnnouncement(view);
    }
    scheduler.invalidate();
  }

  /** Live button / L key: back to the scrolling live window. */
  export function returnToLive(): void {
    gesture({ kind: "live" });
  }

  function pickWindow(id: LiveWindowId): void {
    const wasLive = view.mode === "live";
    liveWindow = id;
    view = { mode: "live", window: id };
    leaveHistory();
    sessionGate.reset();
    if (!wasLive) announcement = rangeAnnouncement(view);
  }

  function onKeydown(e: KeyboardEvent): void {
    if (e.ctrlKey || e.metaKey || e.altKey || !CHART_KEYS.includes(e.key)) return;
    e.preventDefault();
    gesture({ kind: "key", key: e.key });
  }

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
    const opts = chartOptions({ theme, visibility, zeroBased, ...sizeOf(el) });
    opts.hooks = {
      ...opts.hooks,
      setCursor: [
        ...(opts.hooks?.setCursor ?? []),
        (self: uPlot) => {
          const left = self.cursor.left;
          hoverLeftPx = left === undefined || left < 0 ? null : left;
          hoverAtMs = hoverLeftPx === null ? null : self.posToVal(hoverLeftPx, "x");
          updateHover();
        },
      ],
      setSelect: [
        ...(opts.hooks?.setSelect ?? []),
        (self: uPlot) => {
          const { left, width } = self.select;
          if (width <= 0) return;
          const t0Ms = self.posToVal(left, "x");
          const t1Ms = self.posToVal(left + width, "x");
          self.setSelect({ left: 0, top: 0, width: 0, height: 0 }, false);
          gesture({ kind: "select", t0Ms, t1Ms });
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

    // Wheel zooms around the pointer, but only when the chart is focused or Ctrl / ⌘
    // is held (trackpad pinch); a plain wheel keeps scrolling the page.
    const onWheel = (e: WheelEvent): void => {
      const focused = document.activeElement === el;
      if (!wheelZooms({ focused, ctrlKey: e.ctrlKey, metaKey: e.metaKey })) return;
      e.preventDefault();
      gesture({ kind: "wheel", atMs: u.posToVal(e.offsetX, "x"), deltaY: e.deltaY });
    };

    // Shift-drag or middle-button drag pans (captured before uPlot's drag-select).
    let panFromX: number | null = null;
    const onPanMove = (e: MouseEvent): void => {
      if (panFromX === null) return;
      const width = over.clientWidth || 1;
      const range = currentRange();
      if (range === null) return;
      const deltaMs = (-(e.clientX - panFromX) / width) * (range[1] - range[0]);
      panFromX = e.clientX;
      if (deltaMs !== 0) gesture({ kind: "pan", deltaMs });
    };
    const onPanEnd = (): void => {
      panFromX = null;
      window.removeEventListener("mousemove", onPanMove);
      window.removeEventListener("mouseup", onPanEnd);
    };
    const onPanStart = (e: MouseEvent): void => {
      if (!(e.button === 1 || (e.button === 0 && e.shiftKey))) return;
      if (!over.contains(e.target as Node)) return;
      e.preventDefault();
      e.stopPropagation();
      panFromX = e.clientX;
      window.addEventListener("mousemove", onPanMove);
      window.addEventListener("mouseup", onPanEnd);
    };
    over.addEventListener("wheel", onWheel, { passive: false });
    el.addEventListener("mousedown", onPanStart, true);

    const observer = new ResizeObserver(() => {
      u.setSize(sizeOf(el));
      scheduler.invalidate();
    });
    observer.observe(el);
    // Draw now, so a rebuilt plot (legend toggle, theme change) never flashes empty;
    // untracked, so drawing does not make this effect depend on the view.
    untrack(draw);
    return () => {
      over.removeEventListener("wheel", onWheel);
      el.removeEventListener("mousedown", onPanStart, true);
      onPanEnd();
      observer.disconnect();
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
          aria-pressed={windowPressed(view, w.id)}
          class:primary={windowPressed(view, w.id)}
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
    <button
      type="button"
      class="live"
      class:primary={view.mode === "live"}
      aria-pressed={view.mode === "live"}
      title="Return the chart to live (L)"
      onclick={returnToLive}>Live</button
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
      <span class="hover-time">{formatSessionSeconds(hover.t_ms, 3)}</span>
      {#each hover.values as v (v.key)}
        <span class="hover-value">
          <span class="hover-label">{seriesDef(v.key)?.label ?? v.key}</span>
          {hoverText(v)}
        </span>
      {/each}
    {/if}
  </div>
  <!-- The plot is a focusable keyboard-driven widget (arrow keys pan / zoom), which
       svelte treats `role="application"` as non-interactive for. -->
  <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
  <div
    class="plot"
    bind:this={host}
    role="application"
    tabindex="0"
    aria-label="Chart. Drag to zoom, Shift-drag to pan; the wheel zooms when the chart is focused or with Ctrl or ⌘ held; arrow keys pan and zoom, L returns to live."
    onkeydown={onKeydown}
  >
    {#if historyError !== null && view.mode === "history"}
      <p class="history-error" role="status">{historyError}</p>
    {/if}
  </div>
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
  .hover-time,
  .hover-label {
    color: var(--muted, GrayText);
  }
  .hover-label {
    margin-right: 0.25rem;
  }
  .plot:focus-visible {
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
  .plot {
    flex: 1;
    min-width: 0;
    min-height: 12rem;
    position: relative;
    overflow: hidden;
  }
</style>
