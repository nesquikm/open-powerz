<script lang="ts">
  import uPlot from "uplot";
  import { untrack } from "svelte";
  import { toAlignedData, type ChartFrame } from "../chartData";
  import {
    CHART_SERIES,
    chartOptions,
    defaultVisibility,
    toggleSeries,
    type ChartSeriesKey,
    type SeriesVisibility,
  } from "../chartSeries";
  import type { SharedInsets } from "../plotInsets";
  import { THEME_COLORS, type ResolvedTheme } from "../theme";
  import type { TimeWarp } from "../timeline";
  import SeriesLegend from "./SeriesLegend.svelte";

  /** What the sub-chart drew last: its x range and playhead, and the playhead's x in the
   * viewport (AC-AXHK2T.6). */
  interface SubChartDraw {
    t0Ms: number;
    t1Ms: number;
    playheadMs: number | null;
    playheadX: number | null;
  }

  /** A second plot under the main one (AC-AXHK2T.6): its own series choice, drawn from
   * the main chart's frame on the main chart's time range, warp and playhead. It keeps
   * no data of its own; the main chart hands it each draw. */
  let {
    theme,
    zeroBased,
    pxRatio,
    reverseCurrent = false,
    insets,
    originMs,
    warp,
    splits,
    onresize,
  }: {
    theme: ResolvedTheme;
    zeroBased: boolean;
    pxRatio: number;
    /** The current axis upside down, as on the main plot (AC-AXHK2T.7). */
    reverseCurrent?: boolean;
    /** Pads this plot to the main plot's plot-area left and right. */
    insets: SharedInsets;
    originMs: () => number;
    /** The main chart's time warp, so long gaps collapse here as there. */
    warp: () => TimeWarp;
    /** The main chart's time-axis ticks. */
    splits: (u: uPlot, min: number, max: number, incr: number) => number[];
    /** The plot was (re)built or resized: the main chart redraws. */
    onresize: () => void;
  } = $props();

  let visibility: SeriesVisibility = $state(defaultVisibility());
  let host: HTMLDivElement | undefined = $state();
  let plot: uPlot | null = null;
  /** The last draw, replayed when the plot is rebuilt. */
  let last: {
    frame: ChartFrame;
    range: [number, number] | null;
    playheadMs: number | null;
  } | null = null;

  /** Draws `frame` on `range` with the playhead at `playheadMs`; returns what it drew. */
  export function draw(
    frame: ChartFrame,
    range: [number, number] | null,
    playheadMs: number | null,
  ): SubChartDraw | null {
    last = { frame, range, playheadMs };
    const u = plot;
    if (u === null) return null;
    u.batch(() => {
      u.setData(toAlignedData(frame), range === null);
      if (range !== null) u.setScale("x", { min: range[0], max: range[1] });
    });
    const min = u.scales.x?.min;
    const max = u.scales.x?.max;
    if (min === undefined || max === undefined || min === null || max === null) return null;
    const playheadX =
      playheadMs === null
        ? null
        : u.over.getBoundingClientRect().left + u.valToPos(playheadMs, "x");
    return { t0Ms: min, t1Ms: max, playheadMs, playheadX };
  }

  function sizeOf(el: HTMLElement): { width: number; height: number } {
    return { width: Math.max(1, el.clientWidth), height: Math.max(1, el.clientHeight) };
  }

  // (Re)build on the host, the theme, a series toggle or the axes mode, as the main plot.
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
      ...sizeOf(el),
    });
    opts.scales = {
      ...opts.scales,
      x: {
        ...opts.scales?.x,
        distr: 100,
        fwd: (v: number) => warp().fwd(v),
        bwd: (v: number) => warp().bwd(v),
      },
    };
    const timeAxis = opts.axes?.[0];
    if (timeAxis) {
      timeAxis.splits = (self: uPlot, _i: number, min: number, max: number, incr: number) =>
        splits(self, min, max, incr);
      timeAxis.filter = (_u: uPlot, s: number[]) => s;
    }
    // Only the main plot takes gestures; the sub-chart follows it.
    opts.cursor = { drag: { x: false, y: false, setScale: false }, bind: { dblclick: () => null } };
    const palette = THEME_COLORS[theme];
    opts.hooks = {
      ...opts.hooks,
      draw: [
        ...(opts.hooks?.draw ?? []),
        // The main chart's playhead, the shared cursor.
        (self: uPlot) => {
          const head = last?.playheadMs ?? null;
          const min = self.scales.x?.min;
          const max = self.scales.x?.max;
          if (head === null || min == null || max == null || head < min || head > max) return;
          const x = Math.round(self.valToPos(head, "x", true));
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
    };
    const shared = insets.attach("sub", () => {
      if (plot === u) {
        u.redraw(false, true);
        onresize();
      }
    });
    opts.padding = shared.padding;
    const empty: uPlot.AlignedData = [[], ...CHART_SERIES.map(() => [])];
    const u = new uPlot(opts, last === null ? empty : toAlignedData(last.frame), el);
    plot = u;
    shared.bind(u);
    let resizeFrame = 0;
    const observer = new ResizeObserver(() => {
      if (resizeFrame) return;
      resizeFrame = requestAnimationFrame(() => {
        resizeFrame = 0;
        u.setSize(sizeOf(el));
        onresize();
      });
    });
    observer.observe(el);
    untrack(() => {
      if (last !== null) draw(last.frame, last.range, last.playheadMs);
      onresize();
    });
    return () => {
      observer.disconnect();
      if (resizeFrame) cancelAnimationFrame(resizeFrame);
      shared.detach();
      u.destroy();
      if (plot === u) plot = null;
    };
  });

  function toggle(key: ChartSeriesKey): void {
    visibility = toggleSeries(visibility, key);
  }
</script>

<section class="sub-chart" aria-label="Sub-chart">
  <SeriesLegend {theme} {visibility} onToggle={toggle} label="Sub-chart series" />
  <div class="sub-plot" bind:this={host}></div>
</section>

<style>
  .sub-chart {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    flex: 1;
    min-width: 0;
    min-height: 10rem;
  }
  .sub-plot {
    flex: 1;
    min-width: 0;
    min-height: 8rem;
    position: relative;
    overflow: hidden;
  }
</style>
