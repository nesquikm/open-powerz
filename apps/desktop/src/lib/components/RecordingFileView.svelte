<script lang="ts">
  import { untrack } from "svelte";
  import uPlot from "uplot";
  import "uplot/dist/uPlot.min.css";
  import { recordingFileRange, type FileSummary } from "../backend";
  import { bucketsToFrame, toAlignedData, type ChartFrame } from "../chartData";
  import {
    CHART_SERIES,
    RECORDING_VISIBILITY,
    chartOptions,
    followRecordingPlotSize,
    formatSessionTime,
    recordingPlotSize,
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
  import type { ResolvedTheme } from "../theme";
  import SeriesLegend from "./SeriesLegend.svelte";

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
    const opts = chartOptions({ theme, visibility, ...recordingPlotSize(node) });
    opts.hooks = {
      ...opts.hooks,
      // The plot area has its real width here: a resize asks again at that width.
      setSize: [
        ...(opts.hooks?.setSize ?? []),
        (self: uPlot) => {
          if (self === plot && self.over.clientWidth !== askedWidth) ask();
        },
      ],
    };
    const u = new uPlot(opts, emptyData(), node);
    plot = u;
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
      <span class="range" role="status">{rangeText}</span>
      <button type="button" disabled={shown === null} onclick={showWholeFile}>Whole file</button>
    </div>
  </div>

  <div class="chart" bind:this={el} aria-label="Recording chart" role="img"></div>

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
