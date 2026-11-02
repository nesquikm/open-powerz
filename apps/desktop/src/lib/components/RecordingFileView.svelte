<script lang="ts">
  import { untrack } from "svelte";
  import uPlot from "uplot";
  import "uplot/dist/uPlot.min.css";
  import {
    recordingFileRange,
    recordingFileRangeStats,
    recordingFileRunProgress,
    type FileSummary,
  } from "../backend";
  import { bucketsToFrame, toAlignedData, type ChartFrame } from "../chartData";
  import {
    CHART_SERIES,
    RECORDING_VISIBILITY,
    chartOptions,
    followRecordingPlotSize,
    formatSessionTime,
    recordingPlotSize,
    timeSplits,
    toggleSeries,
    type ChartSeriesKey,
    type SeriesVisibility,
  } from "../chartSeries";
  import {
    HistoryRequester,
    historyRequestFor,
    rangeDecimals,
    sessionRequestFor,
    type TimeRange,
  } from "../chartView";
  import { rowsText } from "../recording.svelte";
  import { statsView } from "../stats";
  import { THEME_COLORS, type ResolvedTheme } from "../theme";
  import {
    ED_DEFINITION,
    energySharePointCount,
    extrasNotes,
    marksInRange,
    edColor,
    edLabelBox,
    paintEnergyShare,
    paintSegmentMarks,
    type MarkKind,
  } from "../chartExtras";
  import { RulerState } from "../rulerState.svelte";
  import { chartExtras } from "../chartExtrasStore.svelte";
  import type { RecordingExtrasState } from "../../../e2e/hook";
  import { E2E_BUILD, installRecordingProbe } from "../e2eHook";
  import { SharedInsets } from "../plotInsets";
  import { RunProgressFeed } from "../runProgressFeed";
  import { buildWarp } from "../timeline";
  import ChartOptions from "./ChartOptions.svelte";
  import ChartRuler from "./ChartRuler.svelte";
  import RulerReadout from "./RulerReadout.svelte";
  import SeriesLegend from "./SeriesLegend.svelte";
  import SubChart from "./SubChart.svelte";

  /** A recording opened from a file (FR AXHK2S): its chart and statistics, read-only. A
   * plot of its own, so the live chart and its history are never touched. */
  let {
    summary,
    theme,
    onClose,
    onError,
  }: {
    summary: FileSummary;
    theme: ResolvedTheme;
    onClose: () => void;
    /** A bucket read failed. */
    onError: (err: unknown) => void;
  } = $props();

  /** Starts as an offline record does. The legend's choice stays in this view: the live
   * chart's stored one is never touched. */
  let visibility: SeriesVisibility = $state.raw(RECORDING_VISIBILITY);
  /** Past any time a record holds, so the whole file is asked for from wherever it starts. */
  const WHOLE_FILE_MS = 2 ** 32;
  /** A shorter drag is a slipped click, not a zoom. */
  const MIN_DRAG_PX = 2;

  let el: HTMLDivElement | undefined = $state();

  const view = $derived(statsView(summary.stats));
  const rows = $derived([
    { name: "VBUS", values: view.vbus },
    { name: "IBUS", values: view.ibus },
    { name: "Power", values: view.power },
  ]);

  /** The chart options (FR AXHK2T), the same ones the live chart reads and saves
   * (AC-AXHK2T.8); each read on its own, so only Reverse current rebuilds the plot. */
  let optionsOpen = $state(false);
  const rulerOn = $derived(chartExtras.value.ruler);
  /** Reverse current: the current axis drawn upside down, the values unchanged (AC-AXHK2T.7). */
  const reverseCurrent = $derived(chartExtras.value.reverseCurrent);
  const marks: MarkKind = $derived(chartExtras.value.marks);
  const markStep = $derived(chartExtras.value.markStep);
  const edOn = $derived(chartExtras.value.ed);
  const subChartOn = $derived(chartExtras.value.subChart);
  /** The ruler over the file's raw rows (AC-AXHK2T.1). */
  const ruler = new RulerState(recordingFileRangeStats);
  let rulerLayer: ChartRuler | undefined = $state();
  /** The file's first and last record times (not bucket times, so the last records stay
   * in reach); empty until the backend says. */
  let fileExtent: TimeRange = $state.raw({ t0Ms: 0, t1Ms: 0 });
  /** Bumped when another file replaces this one: answers for the old one are dropped. */
  let fileGeneration = 0;

  /** Segment marks and E.D. over the file's raw rows (AC-AXHK2T.4, .5); an answer redraws. */
  const progress = new RunProgressFeed(recordingFileRunProgress, () => {
    progressTotals = { marks: progress.marksTotalPositive, energy: progress.energyTotalPositive };
    plot?.redraw(false);
    probeDraw();
  });
  $effect(() => () => progress.dispose());
  let progressTotals: { marks: boolean | null; energy: boolean | null } = $state({
    marks: null,
    energy: null,
  });
  const extrasNoteText = $derived(extrasNotes({ marks, ed: edOn }, progressTotals).join(" "));

  /** Asks for the marks and E.D. of the range on screen, if anything changed. */
  function wantProgress(): void {
    const u = plot;
    const min = u?.scales.x?.min;
    const max = u?.scales.x?.max;
    if (u == null || min == null || max == null || (marks === "off" && !edOn)) {
      progress.want(null);
      return;
    }
    progress.want({
      marks,
      step: markStep,
      ed: edOn,
      edPoints: Math.max(2, Math.round(u.over.clientWidth)),
      edT0Ms: min,
      edT1Ms: max,
      stamp: fileGeneration,
    });
  }

  function setMarks(kind: MarkKind, step: number): void {
    chartExtras.update({ marks: kind, markStep: step });
  }

  // New options, from this view or the live chart: asked for and redrawn.
  $effect(() => {
    void [marks, markStep, edOn];
    untrack(() => {
      wantProgress();
      plot?.redraw(false);
    });
  });

  // The ruler turned off, here or on the live chart: this view's goes too.
  $effect(() => {
    if (!rulerOn) untrack(() => ruler.clear());
    else untrack(placeRulerOnScreen);
  });

  function toggleRuler(): void {
    chartExtras.update({ ruler: !rulerOn });
  }

  /** With the ruler on and none placed, places it in the range on screen. */
  function placeRulerOnScreen(): void {
    const min = plot?.scales.x?.min;
    const max = plot?.scales.x?.max;
    if (rulerOn && min != null && max != null) ruler.placeIn(min, max);
  }

  /** "Reset ruler": both cursors back in the range on screen. */
  function resetRuler(): void {
    const min = plot?.scales.x?.min;
    const max = plot?.scales.x?.max;
    if (min != null && max != null) ruler.reset(min, max);
  }

  /** The sub-chart (AC-AXHK2T.6) and the shared plot-area insets that line it up. */
  let subChart: SubChart | undefined = $state();
  const insets = new SharedInsets();
  /** A recording has no collapsed gaps: one ms is one ms of display. */
  const noWarp = buildWarp([], { t0Ms: 0, t1Ms: 0 }, 0);
  const pxRatio = globalThis.devicePixelRatio || 1;
  let subDrawn: { t0Ms: number; t1Ms: number } | null = null;

  /** Test builds only: what the e2e tests read of the last draw. */
  function probeDraw(): void {
    if (!E2E_BUILD) return;
    const u = plot;
    probeState = {
      segmentMarks: marksInRange(progress.marks, u?.scales.x?.min, u?.scales.x?.max).map((m) => ({
        ...m,
      })),
      ampsDir: u?.scales.amps?.dir === -1 ? -1 : 1,
      edPoints: energySharePointCount(progress.ed),
      sub: subChartOn && subDrawn !== null ? { ...subDrawn } : null,
      t0Ms: u?.scales.x?.min ?? 0,
      t1Ms: u?.scales.x?.max ?? 0,
    };
  }
  let probeState: RecordingExtrasState | null = null;
  // The sub-chart turned off: the probe says so without waiting for a draw.
  $effect(() => {
    void subChartOn;
    untrack(probeDraw);
  });
  if (E2E_BUILD) {
    $effect(() =>
      installRecordingProbe(
        () =>
          probeState ?? {
            segmentMarks: [],
            ampsDir: 1,
            edPoints: 0,
            sub: null,
            t0Ms: 0,
            t1Ms: 0,
          },
      ),
    );
  }

  /** The range on screen, null for the whole file; moves with the answer that draws it. */
  let shown: TimeRange | null = $state(null);
  /** The range asked for last, null for the whole file. */
  let wanted: TimeRange | null = null;
  /** The answer on screen, kept for a plot rebuilt by the legend or the theme. */
  let frame: ChartFrame | null = null;
  let plot: uPlot | null = null;
  /** The plot width of the last ask: one bucket per pixel. */
  let askedWidth = 0;

  const rangeText = $derived.by(() => {
    if (shown === null) return "Drag across the chart to zoom in.";
    const decimals = rangeDecimals(shown.t1Ms - shown.t0Ms);
    return `${formatSessionTime(shown.t0Ms, decimals)} to ${formatSessionTime(shown.t1Ms, decimals)}`;
  });

  /** The newest range wins: a stale answer is dropped here, never drawn. */
  const requester = new HistoryRequester({
    fetch: (req) => recordingFileRange(req.series, req.t0Ms, req.t1Ms, req.buckets),
    onResult: (req, result) => {
      frame = bucketsToFrame(result);
      shown = req.purpose === "view" ? { t0Ms: req.t0Ms, t1Ms: req.t1Ms } : null;
      draw();
    },
    onError: (err) => {
      wanted = shown;
      onError(err);
    },
  });

  function draw(): void {
    const u = plot;
    if (u === null || frame === null) return;
    const data = toAlignedData(frame);
    const range = shown;
    u.batch(() => {
      u.setData(data, range === null);
      if (range !== null) u.setScale("x", { min: range.t0Ms, max: range.t1Ms });
    });
    const min = u.scales.x?.min;
    const max = u.scales.x?.max;
    // The sub-chart on the same x range as drawn here.
    subDrawn =
      subChartOn && min != null && max != null
        ? (subChart?.draw(frame, [min, max], null) ?? null)
        : null;
    // Both plot areas on the same x span (AC-AXHK2T.6).
    insets.align();
    wantProgress();
    placeRulerOnScreen();
    rulerLayer?.layout();
    probeDraw();
  }

  /** Asks for the wanted range, one bucket per pixel. */
  function ask(): void {
    askedWidth = plot?.over.clientWidth ?? 0;
    const req =
      wanted === null
        ? sessionRequestFor(WHOLE_FILE_MS, askedWidth)
        : historyRequestFor({ mode: "history", window: "all", ...wanted }, askedWidth);
    if (req !== null) requester.request(req);
  }

  /** A drag over the plot ended: its range, read anew from the backend. */
  function zoomToSelection(u: uPlot, left: number, width: number): void {
    if (width < MIN_DRAG_PX) return;
    const t0Ms = Math.max(0, u.posToVal(left, "x"));
    const t1Ms = u.posToVal(left + width, "x");
    if (!(t1Ms > t0Ms)) return;
    wanted = { t0Ms, t1Ms };
    ask();
  }

  function showWholeFile(): void {
    if (wanted === null) return;
    wanted = null;
    ask();
  }

  function toggle(key: ChartSeriesKey): void {
    visibility = toggleSeries(visibility, key);
  }

  function emptyData(): uPlot.AlignedData {
    return [[], ...CHART_SERIES.map(() => [])];
  }

  // A new theme or a series toggle rebuilds the plot (uPlot cannot show or hide an axis in
  // place); the answer on screen stays, so nothing is read again.
  $effect(() => {
    const node = el;
    if (!node) return;
    const opts = chartOptions({ theme, visibility, reverseCurrent, ...recordingPlotSize(node) });
    const palette = THEME_COLORS[theme];
    opts.hooks = {
      ...opts.hooks,
      draw: [
        ...(opts.hooks?.draw ?? []),
        // Segment marks (AC-AXHK2T.4), then E.D., the energy share (AC-AXHK2T.5). The
        // "E.D. %" label's box is computed once: mark labels give way to it (AC-AXHK2V.15).
        (self: uPlot) => {
          const ed = edLabelBox(self, progress.ed);
          paintSegmentMarks(self, progress.marks, palette.muted, ed ? [ed] : []);
          paintEnergyShare(self, progress.ed, edColor(theme), ed);
        },
      ],
      // The plot area has its real width here: a resize asks again at that width.
      setSize: [
        ...(opts.hooks?.setSize ?? []),
        (self: uPlot) => {
          if (self === plot && self.over.clientWidth !== askedWidth) ask();
          rulerLayer?.layout();
        },
      ],
    };
    // The same plot-area left and right as the sub-chart (AC-AXHK2T.6).
    const shared = insets.attach("main", () => {
      if (plot === u) {
        u.redraw(false, true);
        rulerLayer?.layout();
      }
    });
    opts.padding = shared.padding;
    const u = new uPlot(opts, emptyData(), node);
    plot = u;
    shared.bind(u);
    untrack(draw);
    const over = u.over;
    over.addEventListener("dblclick", showWholeFile);

    // The drag is handled here and captured before uPlot sees it: WebKit, the macOS app's
    // engine, does not reliably reach uPlot's own drag-select (the live chart does the same).
    const plotX = (clientX: number): number => {
      const r = over.getBoundingClientRect();
      return Math.min(Math.max(clientX - r.left, 0), r.width);
    };
    let fromX: number | null = null;
    const onDragMove = (e: MouseEvent): void => {
      // No button held: the mouseup went missing, so the drag ends here, nothing applied.
      if (e.buttons === 0) {
        onDragEnd();
        return;
      }
      if (fromX === null) return;
      const x = plotX(e.clientX);
      u.setSelect(
        { left: Math.min(fromX, x), top: 0, width: Math.abs(x - fromX), height: over.clientHeight },
        false,
      );
    };
    const onDragEnd = (e?: MouseEvent): void => {
      if (fromX !== null) {
        const from = fromX;
        fromX = null;
        u.setSelect({ left: 0, top: 0, width: 0, height: 0 }, false);
        const to = e ? plotX(e.clientX) : from;
        zoomToSelection(u, Math.min(from, to), Math.abs(to - from));
      }
      window.removeEventListener("mousemove", onDragMove);
      window.removeEventListener("mouseup", onDragEnd);
      window.removeEventListener("blur", onDragCancel);
    };
    /** The window lost focus mid-drag: its mouseup may never come, so cancel. */
    const onDragCancel = (): void => {
      fromX = null;
      u.setSelect({ left: 0, top: 0, width: 0, height: 0 }, false);
      onDragEnd();
    };
    const onDragStart = (e: MouseEvent): void => {
      if (e.button !== 0 || !over.contains(e.target as Node)) return;
      e.preventDefault();
      e.stopPropagation();
      fromX = plotX(e.clientX);
      window.addEventListener("mousemove", onDragMove);
      window.addEventListener("mouseup", onDragEnd);
      window.addEventListener("blur", onDragCancel);
    };
    node.addEventListener("mousedown", onDragStart, true);

    const stopFollowing = followRecordingPlotSize(u, node);
    return () => {
      stopFollowing();
      node.removeEventListener("mousedown", onDragStart, true);
      onDragCancel();
      over.removeEventListener("dblclick", showWholeFile);
      plot = null;
      shared.detach();
      u.destroy();
    };
  });

  // A file that replaces this one starts on the whole of it; an answer still on its way
  // for the old one is dropped.
  $effect(() => {
    void summary;
    if (!el) return;
    wanted = null;
    shown = null;
    frame = null;
    const generation = ++fileGeneration;
    fileExtent = { t0Ms: 0, t1Ms: 0 };
    untrack(() => {
      ruler.clear();
      progress.clear();
      progressTotals = { marks: null, energy: null };
    });
    // The file's first and last record: where the ruler's cursors may go.
    recordingFileRunProgress({ kind: "charge", step_pct: 0, ed_points: 0 })
      .then((p) => {
        if (generation === fileGeneration && p !== null) {
          fileExtent = { t0Ms: p.session_t0_ms, t1Ms: p.run_t1_ms };
        }
      })
      .catch((e: unknown) => console.warn("open-powerz: file extent failed", e));
    ask();
    return () => requester.cancel();
  });
</script>

<section class="recording-file" aria-label="Recording file">
  <header class="head">
    <div class="title">
      <h2>{summary.name}</h2>
      <p class="mark">Opened from a file, read-only. Nothing here reaches a meter.</p>
    </div>
    <button type="button" onclick={onClose}>Close file</button>
  </header>

  <p class="counts">
    {rowsText(summary.rows)}{#if summary.skipped > 0}, {summary.skipped.toLocaleString("en-US")}
      damaged {summary.skipped === 1 ? "row" : "rows"} skipped{/if}{#if summary.left_out > 0}, {summary.left_out.toLocaleString(
        "en-US",
      )} more left out past what the view holds{/if}
  </p>

  <div class="controls">
    <SeriesLegend {theme} {visibility} onToggle={toggle} />
    <div class="zoom">
      <button type="button" aria-expanded={optionsOpen} onclick={() => (optionsOpen = !optionsOpen)}
        >Chart options</button
      >
      <span class="range" role="status">{rangeText}</span>
      <button type="button" disabled={shown === null} onclick={showWholeFile}>Whole file</button>
    </div>
  </div>

  {#if optionsOpen}
    <ChartOptions
      extras={chartExtras.value}
      onRuler={toggleRuler}
      onMarks={setMarks}
      onEd={() => chartExtras.update({ ed: !edOn })}
      onSubChart={() => chartExtras.update({ subChart: !subChartOn })}
      onReverseCurrent={() => chartExtras.update({ reverseCurrent: !reverseCurrent })}
    />
  {/if}
  {#if extrasNoteText !== ""}
    <p class="extras-note" role="status">{extrasNoteText}</p>
  {/if}
  {#if edOn}
    <p class="ed-note">
      <span class="ed-swatch" style:border-color={edColor(theme)}></span>
      {ED_DEFINITION}
    </p>
  {/if}
  {#if rulerOn && ruler.ruler !== null}
    <RulerReadout items={ruler.readout} announcement={ruler.announcement} onReset={resetRuler} />
  {/if}

  <!-- The ruler's cursors ride over the chart, outside its image role. -->
  <div class="chart-area">
    <div class="chart" bind:this={el} aria-label="Recording chart" role="img"></div>
    {#if rulerOn}
      <ChartRuler bind:this={rulerLayer} {ruler} plot={() => plot} bounds={() => fileExtent} />
    {/if}
  </div>
  {#if subChartOn}
    <!-- Drawn on a microtask: on its first build the sub-chart is not bound here yet. -->
    <SubChart
      bind:this={subChart}
      {theme}
      zeroBased={false}
      {pxRatio}
      {reverseCurrent}
      {insets}
      originMs={() => 0}
      warp={() => noWarp}
      splits={(_u, min, max, incr) => timeSplits(min, max, incr, 0)}
      onresize={() => queueMicrotask(draw)}
    />
  {/if}

  <dl class="totals">
    <div>
      <dt>Elapsed</dt>
      <dd>{view.elapsed}</dd>
    </div>
    <div>
      <dt>Charge</dt>
      <dd>{view.charge}</dd>
    </div>
    <div>
      <dt>Energy</dt>
      <dd>{view.energy}</dd>
    </div>
  </dl>

  <table>
    <thead>
      <tr>
        <th scope="col"><span class="visually-hidden">Quantity</span></th>
        <th scope="col">Min</th>
        <th scope="col">Max</th>
        <th scope="col">Avg</th>
      </tr>
    </thead>
    <tbody>
      {#each rows as row (row.name)}
        <tr>
          <th scope="row">{row.name}</th>
          <td>{row.values.min}</td>
          <td>{row.values.max}</td>
          <td>{row.values.avg}</td>
        </tr>
      {/each}
    </tbody>
  </table>
</section>

<style>
  .recording-file {
    display: grid;
    gap: 0.75rem;
    min-width: 0;
    font-variant-numeric: tabular-nums;
  }
  .head {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 0.75rem;
  }
  .title {
    min-width: 0;
  }
  .head button {
    flex: none;
    white-space: nowrap;
  }
  h2 {
    margin: 0;
    font-size: 1rem;
    overflow-wrap: anywhere;
  }
  p {
    margin: 0;
  }
  .mark,
  dt,
  thead th,
  tbody th {
    color: var(--muted);
  }
  .mark,
  .counts,
  .range {
    font-size: 0.875rem;
  }
  .range {
    color: var(--muted);
  }
  .controls {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem 1rem;
  }
  .zoom {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }
  .chart-area {
    position: relative;
    min-width: 0;
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
  .chart {
    min-width: 0;
    height: 24rem;
    overflow: hidden;
  }
  .totals {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem 2rem;
    margin: 0;
  }
  dt {
    font-size: 0.8rem;
  }
  dd {
    margin: 0;
    font-weight: 600;
  }
  table {
    width: 100%;
    max-width: 32rem;
    border-collapse: collapse;
    font-size: 0.875rem;
  }
  th,
  td {
    padding: 0.2rem 0;
    text-align: right;
    white-space: nowrap;
  }
  thead th {
    font-weight: 400;
  }
  tbody th {
    text-align: left;
    font-weight: 400;
  }
</style>
