<script lang="ts">
  import uPlot from "uplot";
  import "uplot/dist/uPlot.min.css";
  import type { OfflineRecord } from "../backend";
  import { CHART_SERIES, chartOptions, type SeriesVisibility } from "../chartSeries";
  import { recordChartData } from "../records.svelte";
  import type { ResolvedTheme } from "../theme";

  /** A downloaded offline record drawn with the live chart's series, axes and colours
   * (FR 1HHT00, AC .5); a plot of its own, so the live history is never touched. */
  let { record, theme }: { record: OfflineRecord; theme: ResolvedTheme } = $props();

  /** VBUS, IBUS and power: the channels a record holds. */
  const VISIBILITY = Object.fromEntries(
    CHART_SERIES.map((s) => [s.key, s.key === "vbus" || s.key === "ibus" || s.key === "power"]),
  ) as SeriesVisibility;

  let el: HTMLDivElement | undefined = $state();

  function sizeOf(node: HTMLElement): { width: number; height: number } {
    return { width: Math.max(node.clientWidth, 100), height: Math.max(node.clientHeight, 100) };
  }

  /** x in ms (the chart's unit), then one column per chart series. */
  function alignedData(rec: OfflineRecord): uPlot.AlignedData {
    const [t, v, i, p] = recordChartData(rec);
    const columns: Record<string, number[]> = { vbus: v, ibus: i, power: p };
    const empty = t.map(() => null);
    return [t.map((s) => s * 1000), ...CHART_SERIES.map((s) => columns[s.key] ?? empty)];
  }

  $effect(() => {
    const node = el;
    if (!node) return;
    const u = new uPlot(
      chartOptions({ theme, visibility: VISIBILITY, ...sizeOf(node) }),
      alignedData(record),
      node,
    );
    let frame = 0;
    const observer = new ResizeObserver(() => {
      if (frame) return;
      frame = requestAnimationFrame(() => {
        frame = 0;
        u.setSize(sizeOf(node));
      });
    });
    observer.observe(node);
    return () => {
      observer.disconnect();
      if (frame) cancelAnimationFrame(frame);
      u.destroy();
    };
  });
</script>

<div class="record-chart" bind:this={el} aria-label="Record chart" role="img"></div>

<style>
  .record-chart {
    min-width: 0;
    height: 20rem;
    overflow: hidden;
  }
</style>
