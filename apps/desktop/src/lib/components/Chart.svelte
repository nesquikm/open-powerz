<script lang="ts">
  import Term from "./Term.svelte";
  import InfoButton from "./InfoButton.svelte";
  import uPlot from "uplot";
  import "uplot/dist/uPlot.min.css";
  import { getContext, untrack } from "svelte";
  import {
    historyGaps,
    historyFoldBoundary,
    historyRange,
    rangeStats,
    runProgress,
    type Gap,
    type Sample,
  } from "../backend";
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
    liveNeedsFrame,
    LIVE_SMOOTH_MIN_FRAME_MS,
    liveRange,
    pixelSnappedRange,
    smoothingActive,
    smoothLiveEnd,
    smoothLiveRange,
    steppedRange,
    toAlignedData,
    type ChartFrame,
    type GapBand,
    type HoverReadout,
    type LiveEnd,
    type LiveWindowId,
  } from "../chartData";
  import {
    type ChartSeriesKey,
    type SeriesVisibility,
    chartOptions,
    dashGridStartMs,
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
    MAX_REDRAW_HZ,
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
  import { loadChartStyle, saveChartStyle, type ChartStyle } from "../chartStyle";
  import { chartImageName, type SaveImageChoice } from "../chartExport";
  import { chartSvg } from "../chartSvg";
  import { chartPng } from "../chartPng";
  import { saveChartImage } from "../backend";
  import { THEME_COLORS, browserStorage, type MotionSettings, type ResolvedTheme } from "../theme";
  import type { PdCapture } from "../pdCapture.svelte";
  import {
    PD_LANE_PX,
    PD_MARKER_KINDS,
    PD_MARKER_R,
    clusterMarkers,
    pdClusterHalfWidth,
    markerColors,
    hitMarker,
    markersInRange,
    revealTime,
    type PdMarkerCluster,
  } from "../pdTimeline";
  import {
    ED_DEFINITION,
    energySharePointCount,
    extrasNotes,
    marksInRange,
    edColor,
    edLabelBox,
    paintEnergyShare,
    paintSegmentMarks,
    type LabelBox,
    type MarkKind,
  } from "../chartExtras";
  import { chartExtras } from "../chartExtrasStore.svelte";
  import { SharedInsets, hasPlotArea } from "../plotInsets";
  import { capStride, drawLoopCap } from "../drawCap";
  import { FOLD_CHIP_FONT_PX, foldChip, foldChipColors, foldTick } from "../foldTick";
  import { RunProgressFeed } from "../runProgressFeed";
  import { RulerState } from "../rulerState.svelte";
  import ChartOptions from "./ChartOptions.svelte";
  import ChartRuler from "./ChartRuler.svelte";
  import HintText from "./HintText.svelte";
  import RulerReadout from "./RulerReadout.svelte";
  import OverviewStrip, { type EnvelopeSource, type OverviewState } from "./OverviewStrip.svelte";
  import SaveImageDialog from "./SaveImageDialog.svelte";
  import SeriesLegend from "./SeriesLegend.svelte";
  import SubChart from "./SubChart.svelte";

  let {
    theme,
    rateSps = null,
    active = true,
    pd = null,
    onPdMarker,
    history = { historyRange, historyGaps, historyFoldBoundary, rangeStats, runProgress },
  }: {
    theme: ResolvedTheme;
    rateSps?: number | null;
    /** A meter session is open; without one the chart shows how to start (AC-9CFMD3.5). */
    active?: boolean;
    /** The PD capture, drawn as markers over the plot (FR TGE83D). */
    pd?: PdCapture | null;
    /** A PD marker (or count badge) was clicked: its record index (AC-TGE83D.2). */
    onPdMarker?: (index: number) => void;
    /** Where the history comes from: the meter the chart shows (FR 1HHT03); the session
     * the commands without a device id act on by default. */
    history?: {
      historyRange: typeof historyRange;
      historyGaps: typeof historyGaps;
      /** Where the history's full resolution starts (AC-AXHK2W.11). */
      historyFoldBoundary?: typeof historyFoldBoundary;
      /** The ruler's values from the raw samples (FR AXHK2T). */
      rangeStats: typeof rangeStats;
      /** Segment marks and E.D. from the raw samples (FR AXHK2T). */
      runProgress: typeof runProgress;
    };
  } = $props();

  /** The PD marker layer: a canvas of its own over the top of the plot (FR TGE83D). */
  let pdLayer: HTMLCanvasElement | null = null;
  /** The marker clusters of the last draw, for hit tests. */
  let pdClusters: PdMarkerCluster[] = [];
  /** Some PD record is captured: the marker legend shows. */
  const hasPd = $derived(pd !== null && pd.version >= 0 && pd.store.length > 0);

  /** Draws the PD records inside the plot's x range as markers, dense ones merged into
   * count badges (AC-TGE83D.1). */
  function drawPdMarkers(u: uPlot): void {
    const canvas = pdLayer;
    if (canvas === null) return;
    const ratio = globalThis.devicePixelRatio || 1;
    const widthCss = u.over.clientWidth;
    const w = Math.max(1, Math.round(widthCss * ratio));
    const h = Math.round(PD_LANE_PX * ratio);
    if (canvas.width !== w) canvas.width = w;
    if (canvas.height !== h) canvas.height = h;
    const ctx = canvas.getContext("2d");
    if (ctx === null) return;
    ctx.clearRect(0, 0, canvas.width, canvas.height);
    const min = u.scales.x?.min;
    const max = u.scales.x?.max;
    const markers =
      pd === null || min === undefined || max === undefined || widthCss <= 0 || !laidOut(u)
        ? []
        : markersInRange(pd.store, min, max);
    // At most a plot's worth of clusters per frame (AC-C6VR9K.5).
    const clusters: PdMarkerCluster[] = clusterMarkers(
      markers,
      (ms) => u.valToPos(ms, "x"),
      drawLoopCap(w, h),
    );
    pdClusters = clusters;
    const colors = markerColors(theme);
    const palette = THEME_COLORS[theme];
    const y = PD_LANE_PX / 2;
    ctx.save();
    ctx.scale(ratio, ratio);
    ctx.font = "bold 10px system-ui, sans-serif";
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    for (const c of clusters) {
      const color = colors[c.markers[0]!.kind];
      ctx.fillStyle = color;
      ctx.beginPath();
      if (c.count === 1) {
        ctx.arc(c.px, y, PD_MARKER_R, 0, 2 * Math.PI);
        ctx.fill();
      } else {
        const label = String(c.count);
        const half = pdClusterHalfWidth(c);
        ctx.roundRect(c.px - half, 1, 2 * half, PD_LANE_PX - 2, (PD_LANE_PX - 2) / 2);
        ctx.fill();
        ctx.fillStyle = palette.bg;
        ctx.fillText(label, c.px, y);
      }
    }
    ctx.restore();
    if (E2E_BUILD && probe !== null) {
      probe.pdMarkers(
        clusters.map((c) => {
          const first = c.markers[0]!;
          return {
            leftPx: c.px,
            topPx: y,
            count: c.count,
            kind: first.kind,
            color: colors[first.kind],
            ms: first.ms,
            indices: c.markers.map((m) => m.index),
          };
        }),
      );
    }
  }

  /** The PD status series, each drawn as dots on its measurement series' axis and
   * colour, shown and hidden with that series' legend toggle (AC-TGE83D.4). */
  const PD_STATUS_SERIES = ["vbus", "ibus", "cc1", "cc2"] as const;
  /** Side of a PD status dot, CSS px. */
  const PD_STATUS_DOT_PX = 2;

  /** Canvas px of `u` to viewport CSS px, one frame for all: what the e2e probes report
   * (AC-AXHK2V.12, AC-AXHK2V.15, AC-AXHK2W.11). */
  function canvasToViewport(u: uPlot) {
    const at = u.ctx.canvas.getBoundingClientRect();
    const ratio = globalThis.devicePixelRatio || 1;
    return {
      x: (x: number): number => at.left + x / ratio,
      box: <T extends LabelBox>(b: T): T => ({
        ...b,
        left: at.left + b.left / ratio,
        top: at.top + b.top / ratio,
        right: at.left + b.right / ratio,
        bottom: at.top + b.bottom / ratio,
      }),
    };
  }

  /** Draws the PD status blocks inside the plot's x range over the lines: at most about
   * two dots per pixel column per series, so a long capture stays cheap to draw. */
  function drawPdStatus(u: uPlot): void {
    // What the e2e hook reads, counted only while it listens.
    const lengths = E2E_BUILD && probe !== null ? { vbus: 0, ibus: 0, cc1: 0, cc2: 0 } : null;
    const min = u.scales.x?.min;
    const max = u.scales.x?.max;
    const [lo, hi] =
      pd === null || min === undefined || max === undefined || pd.status.length === 0
        ? [0, 0]
        : pd.status.bounds(min, max);
    if (pd !== null && hi > lo) {
      // The buffer's own columns, read between the bounds: nothing is copied per draw.
      const cols = pd.status.columns;
      const ratio = globalThis.devicePixelRatio || 1;
      // About two dots per pixel column, and never past the draw cap (AC-C6VR9K.5).
      const stride = Math.max(
        Math.ceil((hi - lo) / Math.max(1, 2 * u.bbox.width)) || 1,
        capStride(hi - lo, drawLoopCap(u.bbox.width, u.bbox.height)),
      );
      const size = PD_STATUS_DOT_PX * ratio;
      const ctx = u.ctx;
      ctx.save();
      ctx.beginPath();
      ctx.rect(u.bbox.left, u.bbox.top, u.bbox.width, u.bbox.height);
      ctx.clip();
      for (const key of PD_STATUS_SERIES) {
        const def = seriesDef(key);
        if (!visibility[key] || def === undefined || u.scales[def.axis] === undefined) continue;
        const values = cols[key];
        ctx.fillStyle = pdStatusColors[key];
        let n = 0;
        for (let i = lo; i < hi; i += stride) {
          const v = values[i]!;
          if (!Number.isFinite(v)) continue;
          const x = u.valToPos(cols.t[i]!, "x", true);
          const y = u.valToPos(v, def.axis, true);
          ctx.fillRect(x - size / 2, y - size / 2, size, size);
          n++;
        }
        if (lengths !== null) lengths[key] = n;
      }
      ctx.restore();
    }
    if (E2E_BUILD && probe !== null && lengths !== null) probe.pdStatus(lengths);
  }

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
  let historyHint: string | null = $state(null);
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

  /** The chart options (FR AXHK2T), all off by default, shared with an opened recording
   * and kept across restarts (AC-AXHK2T.8). Each is read on its own, so a toggle rebuilds
   * the plot only when it changes how the plot is built (Reverse current). */
  const rulerOn = $derived(chartExtras.value.ruler);
  const marksKind: MarkKind = $derived(chartExtras.value.marks);
  const markStep = $derived(chartExtras.value.markStep);
  const edOn = $derived(chartExtras.value.ed);
  const subChartOn = $derived(chartExtras.value.subChart);
  const reverseCurrent = $derived(chartExtras.value.reverseCurrent);
  let optionsOpen = $state(false);
  /** The "Save image" dialog is open (FR AXHK2V). */
  let saveOpen = $state(false);
  let saveError = $state("");

  /** Renders the chart as the dialog asked and hands it to the backend to save. */
  async function saveImage(choice: SaveImageChoice): Promise<void> {
    saveOpen = false;
    saveError = "";
    const u = plot;
    if (u === null) return;
    const x = u.scales.x;
    const view = { t0Ms: x?.min ?? 0, t1Ms: x?.max ?? 0 };
    try {
      // A PDF is the PNG, wrapped in one page by the backend.
      const bytes =
        choice.format === "svg"
          ? new TextEncoder().encode(
              chartSvg({
                width: choice.width,
                height: choice.height,
                title: chartStyle.title,
                theme,
                white: choice.white,
                style: chartStyle,
                visibility,
                view,
                originMs: originMs(),
                data: u.data as (number | null)[][],
                zeroBased,
                reverseCurrent,
              }),
            )
          : await chartPng({
              width: choice.width,
              height: choice.height,
              title: chartStyle.title,
              white: choice.white,
              theme,
              visibility,
              zeroBased,
              originMs,
              reverseCurrent,
              style: chartStyle,
              view,
              data: u.data,
            });
      await saveChartImage(chartImageName(choice.format, new Date()), choice.format, bytes);
    } catch (e) {
      saveError = `The image was not saved: ${e instanceof Error ? e.message : String(e)}`;
    }
  }
  /** The ruler's cursors and values (AC-AXHK2T.1). */
  const ruler = new RulerState((t0Ms, t1Ms) => history.rangeStats(t0Ms, t1Ms));
  let rulerLayer: ChartRuler | undefined = $state();

  // Any option change, here or on an opened recording, redraws (a paused chart too).
  $effect(() => {
    void [rulerOn, marksKind, markStep, edOn, subChartOn];
    untrack(() => scheduler.invalidate());
  });
  // The ruler turned off, here or on an opened recording: this chart's goes too.
  $effect(() => {
    if (!rulerOn) untrack(() => ruler.clear());
  });

  function toggleRuler(): void {
    chartExtras.update({ ruler: !rulerOn });
  }

  /** New segment marks or step (AC-AXHK2T.4). */
  function setMarks(marks: MarkKind, step: number): void {
    chartExtras.update({ marks, markStep: step });
  }

  /** Reverse current on or off: only the current axis flips (AC-AXHK2T.7). */
  function toggleReverseCurrent(): void {
    chartExtras.update({ reverseCurrent: !reverseCurrent });
  }

  /** E.D. on or off (AC-AXHK2T.5). */
  function toggleEd(): void {
    chartExtras.update({ ed: !edOn });
  }

  /** The sub-chart under the plot (AC-AXHK2T.6), while it is on. */
  let subChart: SubChart | undefined = $state();
  /** The main plot and the sub-chart pad to the same plot-area left and right, so one ms
   * lands at one x in both (AC-AXHK2T.6). */
  const insets = new SharedInsets();

  /** The sub-chart on or off (AC-AXHK2T.6). */
  function toggleSubChart(): void {
    chartExtras.update({ subChart: !subChartOn });
  }

  /** Segment marks and E.D. from the raw samples, asked for at most 4 times a second
   * while live (AC-AXHK2T.4, .5); an answer redraws. */
  const progress = new RunProgressFeed(
    (q) => history.runProgress(q),
    () => {
      progressTotals = {
        marks: progress.marksTotalPositive,
        energy: progress.energyTotalPositive,
      };
      scheduler.invalidate();
    },
  );
  $effect(() => () => progress.dispose());
  /** The last answer's totals, for the notes on why nothing is drawn. */
  let progressTotals: { marks: boolean | null; energy: boolean | null } = $state({
    marks: null,
    energy: null,
  });
  const extrasNoteText = $derived(
    extrasNotes({ marks: marksKind, ed: edOn }, progressTotals).join(" "),
  );

  /** Asks for the marks and E.D. of the range on screen, if anything changed. */
  function wantProgress(range: [number, number] | null): void {
    if (range === null || (marksKind === "off" && !edOn)) {
      progress.want(null);
      return;
    }
    progress.want({
      marks: marksKind,
      step: markStep,
      ed: edOn,
      edPoints: Math.max(2, Math.round(plotWidthPx())),
      edT0Ms: range[0],
      edT1Ms: range[1],
      // New samples (live or paused) may move the marks and E.D.; a clear resets the feed.
      stamp: buffer.lastMs ?? -1,
    });
  }

  /** With the ruler on and none placed, places it in the range on screen. */
  function placeRulerIn(range: [number, number] | null): void {
    if (rulerOn && hasData && range !== null) ruler.placeIn(range[0], range[1]);
  }

  /** "Reset ruler": both cursors back in the range on screen. */
  function resetRuler(): void {
    const range = currentRange();
    if (range !== null) ruler.reset(range[0], range[1]);
  }

  /** Where a ruler cursor may go: the session's extent. */
  function rulerBounds(): TimeRange {
    const range = currentRange() ?? [0, 0];
    const s = sessionExtent(range[0], range[1]);
    return { t0Ms: s.firstMs, t1Ms: s.lastMs };
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

  /** Places the playhead at `ms` and brings it into view: the view stays as it is when
   * `ms` is visible, else the same span centres on it, paused (AC-TGE83D.3). */
  export function revealPlayheadAt(ms: number): void {
    setPlayhead(ms);
    const range = currentRange();
    if (range === null) return;
    const shown = { t0Ms: range[0], t1Ms: range[1] };
    const next = revealTime(shown, ms, sessionExtent(range[0], range[1]));
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
  /** The PD status dots' colours: their measurement series' strokes, looked up once
   * per theme, not per draw. */
  const pdStatusColors = $derived.by(() => {
    const stroke = (key: (typeof PD_STATUS_SERIES)[number]): string =>
      styles.find((st) => st.key === key)?.stroke ?? THEME_COLORS[theme].text;
    return {
      vbus: stroke("vbus"),
      ibus: stroke("ibus"),
      cc1: stroke("cc1"),
      cc2: stroke("cc2"),
    };
  });
  const liveBtn = $derived(liveButton(view));
  /** No session: the Live button reads "Live" but is not pressed (AC-9CFMD3.5). */
  const livePressed = $derived(active && liveBtn.pressed);

  function toggle(key: ChartSeriesKey): void {
    visibility = toggleSeries(visibility, key);
    saveVisibility(visibility, sessionStore());
  }

  /** "Zero-based axes": every value axis includes 0, kept for the session (AC-AH2AEM.9). */
  let zeroBased = $state(loadZeroBased(sessionStore()));

  /** The chart's appearance settings, kept for the app, not per meter (AC-AXHK2V.8). */
  let chartStyle: ChartStyle = $state(loadChartStyle(browserStorage()));

  function setChartStyle(next: ChartStyle): void {
    chartStyle = next;
    saveChartStyle(next, browserStorage());
  }

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

  /** Wall time (performance.now()) the batch carrying the newest sample arrived. */
  let newestArrivedAt: number | null = null;
  /** The smooth live window's end on the last frame (AC-HR2JAG.2). */
  let liveEnd: LiveEnd | null = null;
  /** The smoothed live end the current draw used, or null when it drew none. */
  let liveEndDrawn: number | null = null;

  /** The live window's range; reduced motion advances it in tenth-of-a-span steps
   * instead of scrolling. */
  function shownRange(): [number, number] | null {
    liveEndDrawn = null;
    const range = liveRange(liveWindow, buffer);
    if (range === null) return null;
    if (liveWindow === "all") {
      return sessionStartMs !== null && source() === "session" ? [sessionStartMs, range[1]] : range;
    }
    const ms = windowMs(liveWindow);
    if (ms === null) return range;
    if (motion().chartScroll === "smooth") {
      // The end moves on with the wall clock between batches where one batch moves the
      // plot by at least 2 px; otherwise it sits on the newest sample (AC-HR2JAG.2).
      let smooth = range;
      if (!smoothScrolling(ms)) {
        liveEnd = null;
      } else if (newestArrivedAt !== null) {
        liveEnd = smoothLiveEnd(liveEnd, range[1], newestArrivedAt, performance.now());
        smooth = smoothLiveRange(liveEnd.endMs, ms, buffer.firstMs ?? range[0]);
        liveEndDrawn = liveEnd.endMs;
      }
      return pixelSnappedRange(smooth, ms, plot?.bbox.width ?? 0, buffer.firstMs ?? range[0]);
    }
    return steppedRange(range, ms, buffer.firstMs ?? range[0]);
  }

  /** True when a `spanMs` live window scrolls smoothly and one batch moves it by at
   * least 2 px, so its end is clocked between batches (AC-HR2JAG.2). */
  function smoothScrolling(spanMs: number | null): boolean {
    return (
      motion().chartScroll === "smooth" && spanMs !== null && smoothingActive(spanMs, plotWidthPx())
    );
  }

  /** Plot-area width in CSS pixels, read from the DOM (uPlot's bbox can lag a resize). */
  function plotWidthPx(): number {
    return plot ? plot.over.clientWidth : 0;
  }

  /** The x range on screen right now. */
  function currentRange(): [number, number] | null {
    return view.mode === "history" ? [view.t0Ms, view.t1Ms] : shownRange();
  }

  /** The live frame, refilled in place on every draw (AC-4NSVC9.2). */
  const liveFrame: ChartFrame = emptyFrame();

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
    if (range === null) return emptyFrame();
    // From the dash grid time left of the view, so every dashed series lays its first
    // cell from the cell's start (FR SX1VTC); the extra stretch keeps the same density.
    const widthPx = plotWidthPx() || 1;
    const span = range[1] - range[0];
    const t0 = plot ? dashGridStartMs(range[0], range[1], plot.bbox.width, pxRatio) : range[0];
    const lead = span > 0 && t0 < range[0] ? ((range[0] - t0) / span) * widthPx : 0;
    return buffer.window(t0, range[1], widthPx + lead, liveFrame);
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

  /** True while the host is hidden (another tab shows) or not laid out: uPlot has
   * sized the plot 1 × 1 and a draw would only lay points out far off it (FR C6VR9K).
   * The resize observer draws again once the host has its real size. */
  function hostHidden(): boolean {
    return host === undefined || host.clientWidth === 0 || host.clientHeight === 0;
  }

  /** True once uPlot has laid the plot out. Just after the host shows again it is still
   * 1 × 1 until the resize observer's frame, which draws again (FR C6VR9K). */
  function laidOut(u: uPlot): boolean {
    return hasPlotArea(u.bbox);
  }

  function draw(): void {
    const u = plot;
    if (!u || hostHidden() || !laidOut(u)) return;
    refreshSession();
    const range = currentRange();
    const frame = frameFor(range);
    shownFrame = frame;
    updateZoomSlider(range);
    warp = range === null ? warpFor(0, 1, 0) : warpFor(range[0], range[1], plotWidthPx());
    wantProgress(range);
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
    drawPdMarkers(u);
    // The sub-chart: the same frame on the same range, with the same playhead.
    const subDrawn = subChartOn ? (subChart?.draw(frame, range, playheadMs) ?? null) : null;
    // Both plot areas on the same x span; a change lays them out again (AC-AXHK2T.6).
    insets.align();
    placeRulerIn(range);
    rulerLayer?.layout();
    if (E2E_BUILD && probe !== null) {
      probe.record(frame, {
        mode: view.mode,
        t0Ms: range?.[0] ?? frame.x[0] ?? 0,
        t1Ms: range?.[1] ?? frame.x[frame.x.length - 1] ?? 0,
        fromHistory,
      });
      probe.overlay(drawnTicks, drawnBands);
      probe.segmentMarks(marksInRange(progress.marks, u.scales.x?.min, u.scales.x?.max));
      probe.edPoints(energySharePointCount(progress.ed));
      probe.ampsDir(u.scales.amps?.dir === -1 ? -1 : 1);
      probe.sub(subDrawn);
      probe.playheadX(
        playheadMs === null
          ? null
          : u.over.getBoundingClientRect().left + u.valToPos(playheadMs, "x"),
      );
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
    // Keep the line moving between batches until it has caught up (AC-HR2JAG.3).
    if (smoothingNow()) scheduler.invalidate();
  }

  /** True while the live, visible chart smooth-scrolls behind the newest sample: every
   * redraw then, data-driven or not, is capped at 30 a second (AC-HR2JAG.3). */
  function smoothingNow(): boolean {
    return liveNeedsFrame({
      live: view.mode === "live",
      visible: !hostHidden(),
      smooth: smoothScrolling(windowMs(liveWindow)),
      endMs: liveEndDrawn,
      newestMs: buffer.lastMs,
    });
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

  /** The strip's share of the live buffer, refilled in place on every paint (AC-4NSVC9.2). */
  const stripFrame: ChartFrame = emptyFrame();

  /** The strip's envelope: the history answer up to where the live buffer starts, the
   * buffer after it. */
  function envelopeSources(cols: number): EnvelopeSource[] {
    const out: EnvelopeSource[] = [];
    const bufFirst = buffer.firstMs;
    const bufLast = buffer.lastMs;
    if (sessionFrame !== null) out.push({ frame: sessionFrame, beforeMs: bufFirst ?? Infinity });
    if (bufFirst !== null && bufLast !== null)
      out.push({ frame: buffer.window(bufFirst, bufLast, cols, stripFrame), beforeMs: Infinity });
    return out;
  }

  /** Something has been drawn, so the Live button has a range to pause (AC-DDDYDJ.2). */
  let hasData = $state(false);

  /** Test builds only: what the e2e hook reports (FR AH2AEN). */
  const probe: ChartProbe | null = E2E_BUILD ? new ChartProbe() : null;
  if (E2E_BUILD && probe !== null) $effect(() => installChartProbe(probe));

  const scheduler = new RedrawScheduler(draw, animationFrames, () =>
    smoothingNow() ? LIVE_SMOOTH_MIN_FRAME_MS : 1000 / MAX_REDRAW_HZ,
  );
  $effect(() => () => scheduler.stop());

  /** A batch arrived. */
  export function push(samples: readonly Sample[]): void {
    const newestBefore = buffer.lastMs;
    buffer.push(samples);
    if (buffer.lastMs !== null && buffer.lastMs !== newestBefore)
      newestArrivedAt = performance.now();
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
    fetchBackfill(first, history)
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
    ruler.clear();
    progress.clear();
    progressTotals = { marks: null, energy: null };
    buffer.clear();
    newestArrivedAt = null;
    liveEnd = null;
    backfillArmed = false;
    backfillGeneration++;
    historyGeneration++;
    hasData = false;
    sessionGaps.clear();
    foldBoundaryMs = null;
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

  /** Where the session history's full resolution starts, ms, `null` while nothing is
   * folded: the chart marks it with a faint tick (AC-AXHK2W.11). */
  let foldBoundaryMs: number | null = null;

  /** Takes a fold boundary answered to a query sent at `generation`: dropped when a
   * {@link reset} came since. `true` when it moved the tick. */
  function acceptFoldBoundary(generation: number, fold: number | null): boolean {
    if (generation !== historyGeneration || fold === foldBoundaryMs) return false;
    foldBoundaryMs = fold;
    return true;
  }

  /** How often the live view asks where the fold boundary is, ms: it moves slowly, so
   * about once a second, never once per frame (AC-AXHK2W.11). */
  const FOLD_POLL_MS = 1_000;

  // The live view (live window and "Whole session" while live) knows the boundary too,
  // not only a paused view asking the history. A paused view keeps the one it was drawn
  // with until its own query.
  $effect(() => {
    const query = history.historyFoldBoundary;
    if (query === undefined || !active) return;
    let inFlight = false;
    const timer = setInterval(() => {
      if (inFlight || view.mode !== "live" || !hasData) return;
      inFlight = true;
      const generation = historyGeneration;
      query()
        .then((fold) => {
          if (acceptFoldBoundary(generation, fold)) scheduler.invalidate();
        })
        .catch(() => {})
        .finally(() => {
          inFlight = false;
        });
    }, FOLD_POLL_MS);
    return () => clearInterval(timer);
  });

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
      const [buckets, gaps, fold] = await Promise.all([
        history.historyRange(req.series, req.t0Ms, req.t1Ms, req.buckets),
        // A failed gap query leaves the gaps already known; the buckets still draw.
        history.historyGaps(req.t0Ms, req.t1Ms).catch(() => []),
        // The fold boundary only moves forward; a failed query keeps the last one.
        (history.historyFoldBoundary?.() ?? Promise.resolve(null)).catch(() => foldBoundaryMs),
      ]);
      if (generation === historyGeneration) for (const g of gaps) sessionGaps.push(g);
      // The result's own draw shows it.
      acceptFoldBoundary(generation, fold);
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
      const { message, hint } = (e ?? {}) as { message?: unknown; hint?: unknown };
      historyError = typeof message === "string" ? message : "The history could not be read.";
      historyHint = typeof hint === "string" ? hint : null;
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
      reverseCurrent,
      style: chartStyle,
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
        // The fold boundary: a faint tick saying the data before it is coarser
        // (AC-AXHK2W.11). One tick at most, so the draw stays bounded.
        (self: uPlot) => {
          const ms = foldBoundaryMs;
          const min = self.scales.x?.min;
          const max = self.scales.x?.max;
          const tick =
            ms === null || min === undefined || max === undefined
              ? null
              : foldTick(ms, { t0Ms: min, t1Ms: max }, self.bbox.width);
          if (tick === null || ms === null || min === undefined || max === undefined) {
            if (E2E_BUILD && probe !== null) probe.foldTick(null);
            return;
          }
          // The axis is warped where gaps collapse: place it through the scale.
          const x = self.valToPos(ms, "x", true);
          const ratio = globalThis.devicePixelRatio || 1;
          const { left, top, width, height } = self.bbox;
          const ctx = self.ctx;
          ctx.save();
          ctx.globalAlpha = 0.45;
          ctx.strokeStyle = palette.muted;
          ctx.lineWidth = Math.max(1, ratio);
          ctx.setLineDash([2 * ratio, 3 * ratio]);
          ctx.beginPath();
          ctx.moveTo(x, top);
          ctx.lineTo(x, top + height);
          ctx.stroke();
          // The label on an opaque chip at the top of the plot, on the coarser side.
          ctx.globalAlpha = 1;
          ctx.setLineDash([]);
          ctx.font = `${Math.round(FOLD_CHIP_FONT_PX * ratio)}px system-ui, sans-serif`;
          const chip = foldChip(x, ctx.measureText(tick.label).width, self.bbox, ratio);
          const colors = foldChipColors(theme);
          if (chip !== null) {
            ctx.fillStyle = colors.background;
            ctx.fillRect(chip.left, chip.top, chip.right - chip.left, chip.bottom - chip.top);
            ctx.fillStyle = colors.text;
            ctx.textAlign = "center";
            ctx.textBaseline = "middle";
            ctx.fillText(tick.label, (chip.left + chip.right) / 2, (chip.top + chip.bottom) / 2);
          }
          ctx.restore();
          if (E2E_BUILD && probe !== null) {
            const css = canvasToViewport(self);
            probe.foldTick(
              chip === null
                ? null
                : {
                    ms,
                    t0Ms: min,
                    t1Ms: max,
                    x: css.x(x),
                    label: tick.label,
                    chip: css.box(chip),
                    plot: css.box({ left, top, right: left + width, bottom: top + height }),
                    fontPx: FOLD_CHIP_FONT_PX,
                    color: colors.text,
                    background: colors.background,
                  },
            );
          }
        },
        // The PD status blocks (AC-TGE83D.4).
        (self: uPlot) => drawPdStatus(self),
        // Segment marks (AC-AXHK2T.4), then E.D., the energy share (AC-AXHK2T.5). The
        // "E.D. %" label's box is computed once: mark labels give way to it (AC-AXHK2V.15).
        (self: uPlot) => {
          const ed = edLabelBox(self, progress.ed);
          const painted = paintSegmentMarks(self, progress.marks, palette.muted, ed ? [ed] : []);
          paintEnergyShare(self, progress.ed, edColor(theme), ed);
          if (E2E_BUILD && probe !== null) {
            const css = canvasToViewport(self).box;
            probe.markLabels(painted.markLabels.map(css), painted.tickLabels.map(css));
            probe.edLabel(ed && css(ed));
          }
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
    // The same plot-area left and right as the sub-chart (AC-AXHK2T.6); alone, the
    // margins uPlot would give it anyway.
    const shared = insets.attach("main", () => {
      if (plot === u) {
        u.redraw(false, true);
        scheduler.invalidate();
      }
    });
    opts.padding = shared.padding;
    const u = new uPlot(opts, toAlignedData(emptyFrame()), el);
    plot = u;
    shared.bind(u);
    const over = u.over;
    // The PD marker lane rides on the plot area, so its x matches the plot's.
    const layer = document.createElement("canvas");
    layer.dataset.layer = "pd-markers";
    layer.setAttribute("aria-hidden", "true");
    Object.assign(layer.style, {
      position: "absolute",
      left: "0",
      top: "0",
      width: "100%",
      height: `${PD_LANE_PX}px`,
      pointerEvents: "none",
      zIndex: "1",
    });
    over.appendChild(layer);
    pdLayer = layer;
    /** The plot x under a viewport x, clamped to the plot. */
    const plotX = (clientX: number): number => {
      const rect = over.getBoundingClientRect();
      return Math.min(Math.max(clientX - rect.left, 0), rect.width);
    };
    /** The PD marker or badge under a viewport point, or null (AC-TGE83D.2). */
    const markerAt = (clientX: number, clientY: number): PdMarkerCluster | null => {
      const rect = over.getBoundingClientRect();
      const y = clientY - rect.top;
      if (y < 0 || y > PD_LANE_PX) return null;
      return hitMarker(pdClusters, clientX - rect.left);
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
      const focusPlot = (): void => {
        pointerFocus = true;
        el.focus({ preventScroll: true });
      };
      // A plain click on a PD marker selects its record (a badge: its first one) rather
      // than scrubbing. Ctrl and ⌘ are left out, as the keyboard does: Ctrl-click is the
      // macOS context-menu gesture.
      if (scrub && onPdMarker && !e.ctrlKey && !e.metaKey) {
        const hit = markerAt(e.clientX, e.clientY);
        const first = hit?.markers[0];
        if (first !== undefined) {
          // Like a scrub, the keyboard lands on the plot, so ← / → step the playhead.
          focusPlot();
          onPdMarker(first.index);
          return;
        }
      }
      focusPlot();
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
      shared.detach();
      u.destroy();
      if (plot === u) plot = null;
      if (pdLayer === layer) pdLayer = null;
    };
  });

  // New PD records redraw the markers, live or paused.
  $effect(() => {
    void pd?.version;
    const u = untrack(() => plot);
    if (u !== null) untrack(() => drawPdMarkers(u));
  });

  // New PD status blocks redraw the plot, live or paused.
  $effect(() => {
    void pd?.statusVersion;
    scheduler.invalidate();
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
    <InfoButton key="section.chart" />
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
    <button type="button" aria-expanded={optionsOpen} onclick={() => (optionsOpen = !optionsOpen)}
      >Chart options</button
    >
    <button type="button" disabled={!hasData} onclick={() => (saveOpen = true)}>Save image</button>
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
  {#if optionsOpen}
    <ChartOptions
      extras={chartExtras.value}
      onRuler={toggleRuler}
      onMarks={setMarks}
      onEd={toggleEd}
      onSubChart={toggleSubChart}
      onReverseCurrent={toggleReverseCurrent}
      style={chartStyle}
      onStyle={setChartStyle}
      {theme}
    />
  {/if}
  {#if extrasNoteText !== ""}
    <p class="extras-note" role="status">{extrasNoteText}</p>
  {/if}
  {#if edOn}
    <p class="ed-note">
      <span class="ed-swatch" style:border-color={edColor(theme)}></span>
      <Term key="term.ed">E.D.</Term>{ED_DEFINITION.slice("E.D.".length)}
    </p>
  {/if}
  <SeriesLegend {theme} {visibility} onToggle={toggle} style={chartStyle} />
  {#if hasPd}
    <ul class="pd-legend" aria-label="PD markers">
      {#each PD_MARKER_KINDS as k (k.kind)}
        <li data-kind={k.kind}>
          <span class="swatch" style:background-color={markerColors(theme)[k.kind]}></span>
          {k.label}
        </li>
      {/each}
    </ul>
  {/if}
  <p class="visually-hidden" role="status" aria-live="polite">{announcement}</p>
  {#if rulerOn && ruler.ruler !== null}
    <RulerReadout items={ruler.readout} announcement={ruler.announcement} onReset={resetRuler} />
  {/if}
  {#if saveOpen}
    <SaveImageDialog
      initialWidth={host?.clientWidth ?? 1280}
      initialHeight={host?.clientHeight ?? 720}
      onCancel={() => (saveOpen = false)}
      onSave={(choice) => void saveImage(choice)}
    />
  {/if}
  {#if saveError !== ""}
    <p class="extras-note" role="alert">{saveError}</p>
  {/if}
  {#if chartStyle.title !== ""}
    <!-- Text interpolation: the title is shown as typed, never parsed as markup (AC-AXHK2V.4). -->
    <p class="chart-title">{chartStyle.title}</p>
  {/if}
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
    {:else if !active && hasData}
      <!-- After a disconnect the last session stays on screen to look back at; the note
           takes the readout's line so it covers no axis or trace. -->
      <p class="banner">
        Meter disconnected, showing the last session. Connect a meter to start a new one.
      </p>
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
    {#if rulerOn}
      <ChartRuler
        bind:this={rulerLayer}
        {ruler}
        plot={() => plot}
        bounds={rulerBounds}
        {originMs}
        laneTopPx={PD_LANE_PX}
      />
    {/if}
    {#if !active && !hasData}
      <p class="empty-state">Connect a meter to start</p>
    {/if}
    {#if historyError !== null && view.mode === "history"}
      <p class="history-error" role="status">
        {historyError}
        <HintText hint={historyHint} />
      </p>
    {/if}
  </div>
  {#if subChartOn}
    <SubChart
      bind:this={subChart}
      {theme}
      {zeroBased}
      {pxRatio}
      {reverseCurrent}
      {insets}
      {originMs}
      warp={() => warp}
      splits={warpedSplits}
      onresize={() => scheduler.invalidate()}
    />
  {/if}
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
    gaps={(t0Ms, t1Ms) => sessionGaps.overlapping(t0Ms, t1Ms)}
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
  .extras-note,
  .ed-note {
    margin: 0;
    font-size: 0.8rem;
    color: var(--muted, GrayText);
  }
  .ed-swatch {
    display: inline-block;
    width: 1.2rem;
    border-top: 2px dashed;
    vertical-align: middle;
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
  .pd-legend {
    display: flex;
    flex-wrap: wrap;
    gap: 0.25rem 0.875rem;
    margin: 0;
    padding: 0;
    list-style: none;
    font-size: 0.85em;
    color: var(--muted, GrayText);
  }
  .pd-legend li {
    display: inline-flex;
    align-items: center;
    gap: 0.375rem;
  }
  .pd-legend .swatch {
    width: 0.625rem;
    height: 0.625rem;
    border-radius: 50%;
  }
  .chart-title {
    margin: 0.25rem 0 0;
    text-align: center;
    font-weight: 600;
    overflow-wrap: anywhere;
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
  .banner {
    margin: 0;
    color: var(--muted, GrayText);
  }
  .plot {
    flex: 1;
    min-width: 0;
    min-height: 12rem;
    position: relative;
    overflow: hidden;
  }
</style>
